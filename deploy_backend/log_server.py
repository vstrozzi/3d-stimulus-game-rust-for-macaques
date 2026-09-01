"""Hosting + per-trial logging server for the web game.

Serves the static game bundle (with the COOP/COEP headers SharedArrayBuffer
needs) behind a single password gate, and receives one POST per trial that it
writes to disk in the exact same folder format `controller.py` produces — so
`tools/verify_trial_logs.py` reads it with no changes.

Two passwords (hashed, in env) map to two roles:
  - player → name / instructions / play flow + POST /log
  - admin  → the normal landing (editor) + a file browser to download data

Run (local dev — http://localhost is a secure context, no TLS needed):
    pip install fastapi uvicorn "passlib[argon2]" itsdangerous
    PLAYER_PW_HASH=$(python -c "from passlib.hash import argon2;print(argon2.hash('PLAYERPW'))")
    ADMIN_PW_HASH=$(python -c "from passlib.hash import argon2;print(argon2.hash('ADMINPW'))")
    SECRET_KEY=dev PLAYER_PW_HASH="$PLAYER_PW_HASH" ADMIN_PW_HASH="$ADMIN_PW_HASH" \
        python -m uvicorn deploy_backend.log_server:app --port 8000   # run from repo root

Production puts Caddy in front for HTTPS (see Caddyfile).
"""
import io
import os
import re
import shutil
import tempfile
import time
import zipfile
from collections import defaultdict, deque

from fastapi import FastAPI, Form, HTTPException, Request, Response
from fastapi.responses import FileResponse, JSONResponse, RedirectResponse
from itsdangerous import BadSignature, URLSafeTimedSerializer
from passlib.hash import argon2

# ── Config (env) ────────────────────────────────────────────────────────────
REPO_ROOT = os.path.realpath(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
# Web root served to browsers (deploy_frontend/). Its game_node/assets/
# trials_config entries are symlinks back into the repo, so realpaths resolve
# under REPO_ROOT (the containment check below) — nothing outside the project
# is ever reachable.
FRONTEND_DIR = os.path.realpath(os.environ.get("FRONTEND_DIR", os.path.join(REPO_ROOT, "deploy_frontend")))
LOG_ROOT = os.path.realpath(os.path.join(REPO_ROOT, "out", "server_logs"))
# Trial-config library: uploaded/saved trials, the active default (trials.jsonl)
# and its backup all live here. controller.py / controller_main.js / the editor
# all read trials_config/trials/trials.jsonl as the default.
TRIALS_DIR = os.path.realpath(os.path.join(REPO_ROOT, "trials_config", "trials"))
SECRET_KEY = os.environ.get("SECRET_KEY", "")
PLAYER_PW_HASH = os.environ.get("PLAYER_PW_HASH", "")
ADMIN_PW_HASH = os.environ.get("ADMIN_PW_HASH", "")
COOKIE_NAME = "auth"
COOKIE_MAX_AGE = 24 * 3600  # seconds
# Brute-force throttle: at most LOGIN_MAX attempts per IP per LOGIN_WINDOW s.
LOGIN_MAX = 10
LOGIN_WINDOW = 300

if not (SECRET_KEY and PLAYER_PW_HASH and ADMIN_PW_HASH):
    raise RuntimeError("Set SECRET_KEY, PLAYER_PW_HASH and ADMIN_PW_HASH env vars.")

os.makedirs(LOG_ROOT, exist_ok=True)
_signer = URLSafeTimedSerializer(SECRET_KEY, salt=COOKIE_NAME)
_login_hits = defaultdict(deque)  # ip -> deque[timestamp]

# Paths reachable without a valid cookie. Everything else is gated.
_PUBLIC_PATHS = {"/login", "/favicon.ico"}
# Validates both the per-trial relpath and admin browser paths.
_SAFE_REL = re.compile(r"^[A-Za-z0-9_\-/. ]*$")

app = FastAPI()


# ── Auth helpers ────────────────────────────────────────────────────────────
def _role(request: Request):
    """Return the role in a valid cookie, or None."""
    tok = request.cookies.get(COOKIE_NAME)
    if not tok:
        return None
    try:
        data = _signer.loads(tok, max_age=COOKIE_MAX_AGE)
    except BadSignature:
        return None
    return data.get("role")


@app.middleware("http")
async def gate(request: Request, call_next):
    """Cookie gate on every path + COOP/COEP on every response. This is what
    makes guessing `index.html` / `trial_editor.html` pointless: no valid
    cookie ⇒ redirect (navigations) or 401 (fetch/POST)."""
    path = request.url.path
    role = _role(request)
    if path not in _PUBLIC_PATHS and role is None:
        if request.method == "GET" and "text/html" in request.headers.get("accept", ""):
            resp = RedirectResponse("/login")
        else:
            resp = JSONResponse({"detail": "unauthorized"}, status_code=401)
    elif path.startswith("/admin") and role != "admin":
        resp = JSONResponse({"detail": "forbidden"}, status_code=403)
    else:
        resp = await call_next(request)
    resp.headers["Cross-Origin-Opener-Policy"] = "same-origin"
    resp.headers["Cross-Origin-Embedder-Policy"] = "require-corp"
    return resp


@app.get("/login")
def login_page():
    return FileResponse(os.path.join(FRONTEND_DIR, "login.html"))


@app.post("/login")
def login(request: Request, password: str = Form(...)):
    ip = request.client.host if request.client else "?"
    now = time.time()
    hits = _login_hits[ip]
    while hits and now - hits[0] > LOGIN_WINDOW:
        hits.popleft()
    if len(hits) >= LOGIN_MAX:
        raise HTTPException(429, "Too many attempts, wait a few minutes.")
    hits.append(now)

    if argon2.verify(password, PLAYER_PW_HASH):
        role, dest = "player", "/"
    elif argon2.verify(password, ADMIN_PW_HASH):
        role, dest = "admin", "/?admin=1"
    else:
        raise HTTPException(401, "Wrong password.")

    resp = RedirectResponse(dest, status_code=303)
    resp.set_cookie(
        COOKIE_NAME, _signer.dumps({"role": role}),
        max_age=COOKIE_MAX_AGE, httponly=True, samesite="strict",
        secure=request.url.scheme == "https",
    )
    return resp


@app.get("/me")
def me(request: Request):
    return {"role": _role(request)}


# ── Logging ─────────────────────────────────────────────────────────────────
def _safe_under(root, *parts, contain=None):
    """Join parts under root, rejecting traversal. The resolved realpath must
    stay under `contain` (defaults to `root`). For static files `contain` is
    REPO_ROOT so the frontend's symlinks into the repo are allowed."""
    contain = contain or root
    for p in parts:
        if not p or p.startswith("/") or ".." in p.split("/") or not _SAFE_REL.match(p):
            raise HTTPException(400, "bad path")
    full = os.path.realpath(os.path.join(root, *parts))
    if full != contain and not full.startswith(contain + os.sep):
        raise HTTPException(400, "bad path")
    return full


@app.post("/log")
async def log(request: Request):
    body = await request.json()
    relpath = body.get("relpath", "")
    content = body.get("content", "")
    if not relpath or not isinstance(content, str):
        raise HTTPException(400, "missing fields")
    # Top folder = the server's date, so a day folder holds every player that
    # played that day. `relpath` already starts with <name>/<date>/..., so
    # repeat plays merge under the name and are disambiguated by the existing
    # per-run HHMMSS folder. Compressing any subtree reproduces the previous
    # web-version zip layout.
    path = _safe_under(LOG_ROOT, time.strftime("%Y-%m-%d"), relpath)
    # The 200 is only returned after os.replace, so the client treats it as durable.
    _atomic_write(path, content)
    return {"ok": True}


def _atomic_write(path, content):
    """tmp-file + os.replace, identical to controller.py::_atomic_write_json."""
    os.makedirs(os.path.dirname(path), exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix=".tmp_", dir=os.path.dirname(path))
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            f.write(content)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.remove(tmp)


# ── Admin file browser (admin role enforced in the gate) ────────────────────
@app.get("/admin/list")
def admin_list(path: str = ""):
    base = _safe_under(LOG_ROOT, path) if path else LOG_ROOT
    if not os.path.isdir(base):
        raise HTTPException(404, "not a directory")
    dirs, files = [], []
    for name in sorted(os.listdir(base)):
        full = os.path.join(base, name)
        if os.path.isdir(full):
            dirs.append(name)
        else:
            files.append({"name": name, "size": os.path.getsize(full)})
    return {"path": path, "dirs": dirs, "files": files}


@app.get("/admin/file")
def admin_file(path: str):
    full = _safe_under(LOG_ROOT, path)
    if not os.path.isfile(full):
        raise HTTPException(404, "not found")
    # Served inline as text so it opens in the browser (view, not download).
    return FileResponse(full, media_type="text/plain; charset=utf-8")


@app.post("/admin/rmdir")
async def admin_rmdir(request: Request):
    """Delete a single data subfolder and everything in it. Password-gated;
    refuses to delete the LOG_ROOT itself. Irreversible."""
    body = await request.json()
    if not _check_admin_password(body.get("password", "")):
        raise HTTPException(403, "wrong password")
    rel = body.get("path", "")
    if not rel:
        raise HTTPException(400, "bad path")
    target = _safe_under(LOG_ROOT, rel)
    if target == LOG_ROOT or not os.path.isdir(target):
        raise HTTPException(400, "bad path")
    shutil.rmtree(target)
    return {"ok": True}


@app.get("/admin/zip")
def admin_zip(path: str = ""):
    """Recursively compress the selected folder on the fly (path='' → the whole
    data root). Nothing is stored zipped; compression happens at download time.
    The zip's internal layout matches the previous web version's session ZIP."""
    base = _safe_under(LOG_ROOT, path) if path else LOG_ROOT
    if not os.path.isdir(base):
        raise HTTPException(404, "not a directory")
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as zf:
        for root, _dirs, names in os.walk(base):
            for n in names:
                fp = os.path.join(root, n)
                zf.write(fp, os.path.relpath(fp, base))
    name = (path.rstrip("/").split("/")[-1] or "server_logs") + ".zip"
    return Response(
        buf.getvalue(), media_type="application/zip",
        headers={"Content-Disposition": f'attachment; filename="{name}"'},
    )


# ── Admin presence (how many admins are connected) ──────────────────────────
# In-memory heartbeat map; correct with a single uvicorn worker (our default).
_admin_presence = {}  # client_id -> last_seen epoch
PRESENCE_TTL = 15


@app.get("/admin/presence")
def admin_presence(id: str = ""):
    now = time.time()
    if id:
        _admin_presence[id] = now
    for k in [k for k, t in _admin_presence.items() if now - t > PRESENCE_TTL]:
        _admin_presence.pop(k, None)
    return {"count": len(_admin_presence)}


# ── Trial-config library (admin) ────────────────────────────────────────────
_TRIAL_NAME = re.compile(r"^[A-Za-z0-9_\-.() ]+\.jsonl$")


def _trial_path(name):
    """Validate a trial filename and resolve it directly under TRIALS_DIR."""
    if not name or "/" in name or "\\" in name or ".." in name or not _TRIAL_NAME.match(name):
        raise HTTPException(400, "bad trial name")
    full = os.path.realpath(os.path.join(TRIALS_DIR, name))
    if os.path.dirname(full) != TRIALS_DIR:
        raise HTTPException(400, "bad trial name")
    return full


@app.get("/admin/trials/list")
def trials_list():
    os.makedirs(TRIALS_DIR, exist_ok=True)
    default_path = os.path.join(TRIALS_DIR, "trials.jsonl")
    default_bytes = open(default_path, "rb").read() if os.path.isfile(default_path) else None
    items = []
    for n in sorted(os.listdir(TRIALS_DIR)):
        if not n.endswith(".jsonl"):
            continue
        fp = os.path.join(TRIALS_DIR, n)
        # "default" = same content as the active trials.jsonl, so the promoted
        # trial (not just the trials.jsonl entry) is shown as the default.
        is_default = default_bytes is not None and open(fp, "rb").read() == default_bytes
        items.append({"name": n, "size": os.path.getsize(fp), "is_default": is_default})
    return {"trials": items, "default": "trials.jsonl"}


@app.post("/admin/trials/save")
async def trials_save(request: Request):
    body = await request.json()
    content = body.get("content", "")
    if not isinstance(content, str):
        raise HTTPException(400, "bad content")
    _atomic_write(_trial_path(body.get("name", "")), content)
    return {"ok": True}


@app.post("/admin/trials/delete")
async def trials_delete(request: Request):
    p = _trial_path((await request.json()).get("name", ""))
    if os.path.realpath(p) == os.path.join(TRIALS_DIR, "trials.jsonl"):
        raise HTTPException(400, "cannot delete the default trials.jsonl")
    if os.path.exists(p):
        os.remove(p)
    return {"ok": True}


@app.post("/admin/trials/rename")
async def trials_rename(request: Request):
    body = await request.json()
    src = _trial_path(body.get("old", ""))
    dst = _trial_path(body.get("new", ""))
    if not os.path.isfile(src):
        raise HTTPException(404, "not found")
    os.replace(src, dst)
    return {"ok": True}


@app.post("/admin/trials/make_default")
async def trials_make_default(request: Request):
    """Promote a library trial to the default: back up the current
    trials.jsonl → trials_old_default.jsonl, then copy the selected one into
    trials.jsonl (kept in the library too)."""
    src = _trial_path((await request.json()).get("name", ""))
    if not os.path.isfile(src):
        raise HTTPException(404, "not found")
    default = os.path.join(TRIALS_DIR, "trials.jsonl")
    if os.path.realpath(src) == default:
        return {"ok": True}  # already the default
    if os.path.exists(default):
        os.replace(default, os.path.join(TRIALS_DIR, "trials_old_default.jsonl"))
    shutil.copyfile(src, default)
    return {"ok": True}


def _check_admin_password(pw):
    try:
        return bool(pw) and argon2.verify(pw, ADMIN_PW_HASH)
    except Exception:
        return False


# ── Static bundle (deploy_frontend/, gated by the middleware above) ─────────
def _static_file(request: Request, full: str, media_type=None):
    """Serve a bundle file so the browser always revalidates before reusing it.
    And use up-to-date version
    """
    st = os.stat(full)
    etag = f'"{st.st_mtime_ns:x}-{st.st_size:x}"'
    headers = {"Cache-Control": "no-cache", "ETag": etag}
    if request.headers.get("if-none-match") == etag:
        return Response(status_code=304, headers=headers)
    return FileResponse(full, media_type=media_type, headers=headers)


@app.get("/")
def index(request: Request):
    return _static_file(request, os.path.join(FRONTEND_DIR, "index.html"))


@app.get("/{path:path}")
def static_files(request: Request, path: str):
    full = _safe_under(FRONTEND_DIR, path, contain=REPO_ROOT)
    # The logs are reachable only through the admin-gated /admin/* routes —
    # never as plain static, or any player could read other players' data.
    if full == LOG_ROOT or full.startswith(LOG_ROOT + os.sep):
        raise HTTPException(404, "not found")
    if not os.path.isfile(full):
        raise HTTPException(404, "not found")
    # .gz is served raw (octet-stream, no Content-Encoding) — the client
    # decompresses it itself via DecompressionStream.
    media = ("application/octet-stream" if full.endswith(".gz")
             else "video/mp4" if full.endswith(".mp4")
             else None)
    return _static_file(request, full, media_type=media)

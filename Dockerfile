FROM python:3.12-slim
WORKDIR /app

# Python deps first (layer-cached unless requirements change).
COPY deploy_backend/requirements-server.txt ./deploy_backend/requirements-server.txt
RUN pip install --no-cache-dir -r deploy_backend/requirements-server.txt

# App (wasm + assets are committed, so no Rust build needed). .dockerignore
# keeps target/, out/, data/, .git out of the image.
COPY . .

# Build-time copy of the default trial library — used by the entrypoint to seed
# the (initially empty) trials volume on first run, since the mount shadows the
# committed trials_config/trials/.
RUN cp -r trials_config/trials /app/seed_trials \
    && chmod +x deploy_backend/docker-entrypoint.sh

EXPOSE 8000
ENTRYPOINT ["/app/deploy_backend/docker-entrypoint.sh"]
CMD ["python", "-m", "uvicorn", "deploy_backend.log_server:app", "--host", "0.0.0.0", "--port", "8000"]

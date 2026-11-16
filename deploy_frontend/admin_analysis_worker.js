'use strict';
importScripts('./admin_analysis.js');
let dataset = null;
self.onmessage = ({ data }) => {
  try {
    if (data.type === 'load') {
      dataset = AdminAnalysis.normalize(data.documents, data.sessions);
      const objects = [...new Map(dataset.trials.map(t => {
        const key = `${t.config}|${t.level}|${t.object}`;
        return [key, { key, config: t.config, level: t.level, object: t.object, kind: t.kind, congruency: t.congruency }];
      })).values()];
      self.postMessage({ id: data.id, result: { configs: [...new Set(dataset.trials.map(t => t.config))], levels: [...new Set(dataset.trials.map(t => t.level))].sort((a, b) => a - b), objects } });
    } else {
      if (!dataset) throw new Error('Load sessions first.');
      self.postMessage({ id: data.id, result: AdminAnalysis.figure(dataset, data.options) });
    }
  } catch (error) { self.postMessage({ id: data.id, error: error.message }); }
};

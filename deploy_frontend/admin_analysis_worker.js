'use strict';
importScripts('./admin_analysis.js');
let dataset = null;
self.onmessage = ({ data }) => {
  try {
    if (data.type === 'turns') {                     // Rotation Strategy: loaded only for those plots
      if (!dataset) throw new Error('Load sessions first.');
      dataset.turns = data.payload.turns;
      self.postMessage({ id: data.id, result: true });
    } else if (data.type === 'load') {
      dataset = AdminAnalysis.load(data.payload);
      self.postMessage({ id: data.id, result: AdminAnalysis.meta(dataset) });
    } else {
      if (!dataset) throw new Error('Load sessions first.');
      self.postMessage({ id: data.id, result: AdminAnalysis.figure(dataset, data.options) });
    }
  } catch (error) { self.postMessage({ id: data.id, error: error.message }); }
};

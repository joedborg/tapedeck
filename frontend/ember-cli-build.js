'use strict';

const EmberApp = require('ember-cli/lib/broccoli/ember-app');

module.exports = function (defaults) {
  const app = new EmberApp(defaults, {
    // Fingerprint assets for cache busting in production
    fingerprint: {
      enabled: process.env.EMBER_ENV === 'production',
      extensions: ['js', 'css', 'png', 'jpg', 'gif', 'map', 'svg'],
    },
    'ember-cli-babel': {
      enableTypeScriptTransform: false,
    },
    autoImport: {
      webpack: {
        externals: {},
      },
    },
  });

  return app.toTree();
};

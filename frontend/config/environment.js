'use strict';

module.exports = function (environment) {
  const ENV = {
    modulePrefix: 'tapedeck-ui',
    environment,
    rootURL: '/',
    locationType: 'history',

    EmberENV: {
      EXTEND_PROTOTYPES: false,
      FEATURES: {},
    },

    APP: {
      // The backend API base URL.  In production the Rust server serves both
      // the UI and the API from the same origin, so an empty string means
      // "same origin".  Override with TAPEDECK_API_URL at build time if needed.
      apiBase: process.env.TAPEDECK_API_URL || '',
    },
  };

  if (environment === 'development') {
    ENV.APP.LOG_RESOLVER = false;
    ENV.APP.LOG_ACTIVE_GENERATION = false;
    ENV.APP.LOG_TRANSITIONS = false;
    ENV.APP.LOG_TRANSITIONS_INTERNAL = false;
    ENV.APP.LOG_VIEW_LOOKUPS = false;
    // Point dev UI at the local Rust server
    ENV.APP.apiBase = ENV.APP.apiBase || 'http://localhost:3000';
  }

  if (environment === 'test') {
    ENV.locationType = 'none';
    ENV.APP.LOG_ACTIVE_GENERATION = false;
    ENV.APP.LOG_VIEW_LOOKUPS = false;
  }

  return ENV;
};

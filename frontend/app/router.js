import EmberRouter from '@ember/routing/router';
import config from 'tapedeck-ui/config/environment';

export default class Router extends EmberRouter {
  location = config.locationType;
  rootURL = config.rootURL;
}

Router.map(function () {
  this.route('login');
  this.route('queue');
  this.route('search');
  this.route('settings');
  // Default / redirects to /queue (handled in application route)
});

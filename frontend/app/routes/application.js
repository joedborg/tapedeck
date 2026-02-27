import Route from '@ember/routing/route';
import { service } from '@ember/service';

export default class ApplicationRoute extends Route {
  @service api;
  @service router;
  @service socket;

  beforeModel() {
    // Restore auth state from localStorage
    this.api.restore();
  }

  afterModel() {
    const { currentRouteName } = this.router;
    if (!this.api.isAuthenticated && currentRouteName !== 'login') {
      this.router.transitionTo('login');
    } else if (this.api.isAuthenticated) {
      this.socket.connect();
      if (!currentRouteName || currentRouteName === 'index') {
        this.router.transitionTo('queue');
      }
    }
  }
}

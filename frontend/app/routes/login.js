import Route from '@ember/routing/route';
import { service } from '@ember/service';

export default class LoginRoute extends Route {
  @service api;
  @service router;

  beforeModel() {
    if (this.api.isAuthenticated) {
      this.router.transitionTo('queue');
    }
  }
}

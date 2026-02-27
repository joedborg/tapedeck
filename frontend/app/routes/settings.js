import Route from '@ember/routing/route';
import { service } from '@ember/service';

export default class SettingsRoute extends Route {
  @service api;
  @service router;

  beforeModel() {
    if (!this.api.isAuthenticated) {
      this.router.transitionTo('login');
    }
  }

  async model() {
    const settings = await this.api.fetchSettings();
    // Convert array of {key, value} to a plain object for easy binding
    const map = Object.fromEntries(settings.map(({ key, value }) => [key, value]));
    return { settings, map };
  }

  setupController(controller, model) {
    super.setupController(controller, model);
    controller.map = { ...model.map };
  }
}

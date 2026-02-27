import Route from '@ember/routing/route';
import { service } from '@ember/service';

export default class SearchRoute extends Route {
  @service api;
  @service router;

  beforeModel() {
    if (!this.api.isAuthenticated) {
      this.router.transitionTo('login');
    }
  }

  async model() {
    try {
      const result = await this.api.fetchQueue({ per_page: 1000 });
      return { queuedPids: new Set(result.data.map((item) => item.pid)) };
    } catch {
      return { queuedPids: new Set() };
    }
  }

  setupController(controller, model) {
    super.setupController(controller, model);
    // Seed addedPids with anything currently in the queue so the Queue button
    // stays disabled when navigating back to this page.
    controller.addedPids = new Set([...model.queuedPids, ...controller.addedPids]);
  }
}

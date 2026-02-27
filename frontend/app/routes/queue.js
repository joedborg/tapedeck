import Route from '@ember/routing/route';
import { service } from '@ember/service';

export default class QueueRoute extends Route {
  @service api;
  @service router;

  beforeModel() {
    if (!this.api.isAuthenticated) {
      this.router.transitionTo('login');
    }
  }

  async model() {
    const result = await this.api.fetchQueue({ per_page: 100 });
    return result;
  }

  setupController(controller, model) {
    super.setupController(controller, model);
    // Sync items from the model on every entry (model() runs fresh each visit)
    controller.items = model?.data ?? [];
  }
}

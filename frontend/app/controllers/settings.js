import Controller from '@ember/controller';
import { tracked } from '@glimmer/tracking';
import { action } from '@ember/object';
import { service } from '@ember/service';

export default class SettingsController extends Controller {
  @service api;
  @service router;

  @tracked saved = false;
  @tracked error = null;
  @tracked saving = false;

  // Editable map is populated from model.map via setupController in the route
  @tracked map = {};

  @action
  updateField(key, event) {
    this.map = { ...this.map, [key]: event.target.value };
  }

  @action
  async save(event) {
    event.preventDefault();
    this.saving = true;
    this.error = null;
    try {
      await this.api.bulkUpdateSettings(this.map);
      this.saved = true;
      setTimeout(() => (this.saved = false), 3000);
    } catch (e) {
      this.error = e.message ?? 'Failed to save settings';
    } finally {
      this.saving = false;
    }
  }

  @action
  logout() {
    this.api.logout();
    this.router.transitionTo('login');
  }
}

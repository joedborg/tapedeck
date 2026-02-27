import Controller from '@ember/controller';
import { tracked } from '@glimmer/tracking';
import { action } from '@ember/object';
import { service } from '@ember/service';

export default class LoginController extends Controller {
  @service api;
  @service router;
  @service socket;

  @tracked username = '';
  @tracked password = '';
  @tracked error = null;
  @tracked loading = false;

  @action
  async login(event) {
    event.preventDefault();
    this.error = null;
    this.loading = true;
    try {
      await this.api.login(this.username, this.password);
      this.socket.connect();
      this.router.transitionTo('queue');
    } catch (e) {
      this.error = e.message ?? 'Login failed';
    } finally {
      this.loading = false;
    }
  }

  @action
  updateUsername(event) {
    this.username = event.target.value;
  }

  @action
  updatePassword(event) {
    this.password = event.target.value;
  }
}

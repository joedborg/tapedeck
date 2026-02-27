import Controller from '@ember/controller';
import { tracked } from '@glimmer/tracking';
import { action } from '@ember/object';
import { service } from '@ember/service';

export default class QueueController extends Controller {
  @service api;
  @service socket;
  @service router;

  @tracked items = [];
  @tracked filter = 'all';
  @tracked error = null;

  #pollTimer = null;

  // ── Lifecycle ─────────────────────────────────────────────────────────────

  @action
  setup() {
    // Called from the queue template on insertion
    this.items = this.model?.data ?? [];
    this.socket.on('progress', this.#onProgress);
    this.socket.on('status_change', this.#onStatusChange);
    this.socket.on('error', this.#onError);
    this.socket.on('item_added', this.#onItemAdded);
    this.socket.on('item_removed', this.#onItemRemoved);
    this.#pollTimer = setInterval(() => this.refresh(), 5000);
  }

  @action
  teardown() {
    clearInterval(this.#pollTimer);
    this.#pollTimer = null;
    this.socket.off('progress', this.#onProgress);
    this.socket.off('status_change', this.#onStatusChange);
    this.socket.off('error', this.#onError);
    this.socket.off('item_added', this.#onItemAdded);
    this.socket.off('item_removed', this.#onItemRemoved);
  }

  // ── WS handlers ───────────────────────────────────────────────────────────

  #onProgress = (event) => {
    const item = this.items.find((i) => i.id === event.id);
    if (item) {
      item.progress = event.progress;
      item.speed = event.speed;
      item.eta = event.eta;
      // Trigger Glimmer reactivity by reassigning
      this.items = [...this.items];
    }
  };

  #onStatusChange = (event) => {
    const item = this.items.find((i) => i.id === event.id);
    if (item) {
      item.status = event.status;
      // Clear any stale error message when a new attempt starts or finishes successfully
      if (event.status === 'downloading' || event.status === 'done') {
        item.error = null;
      }
      this.items = [...this.items];
    }
  };

  #onError = (event) => {
    const item = this.items.find((i) => i.id === event.id);
    if (item) {
      item.error = event.message;
      this.items = [...this.items];
    }
  };

  #onItemAdded = (event) => {
    this.items = [event.item, ...this.items];
  };

  #onItemRemoved = (event) => {
    this.items = this.items.filter((i) => i.id !== event.id);
  };

  // ── Computed ──────────────────────────────────────────────────────────────

  get filteredItems() {
    if (this.filter === 'all') return this.items;
    return this.items.filter((i) => i.status === this.filter);
  }

  get statusCounts() {
    const counts = { all: this.items.length, queued: 0, downloading: 0, done: 0, failed: 0 };
    for (const item of this.items) {
      counts[item.status] = (counts[item.status] ?? 0) + 1;
    }
    return counts;
  }

  // ── Actions ───────────────────────────────────────────────────────────────

  @action
  setFilter(f) {
    this.filter = f;
  }

  @action
  async remove(id) {
    try {
      await this.api.removeFromQueue(id);
      // WS item_removed event will update the list, but remove optimistically too
      this.items = this.items.filter((i) => i.id !== id);
    } catch (e) {
      this.error = e.message;
    }
  }

  @action
  async retry(id) {
    try {
      const updated = await this.api.retryDownload(id);
      this.items = this.items.map((i) => (i.id === id ? updated : i));
    } catch (e) {
      this.error = e.message;
    }
  }

  @action
  async refresh() {
    try {
      const result = await this.api.fetchQueue({ per_page: 100 });
      this.items = result.data;
    } catch (e) {
      this.error = e.message;
    }
  }

  @action
  logout() {
    this.api.logout();
    this.socket.disconnect();
    this.router.transitionTo('login');
  }
}

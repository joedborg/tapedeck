import Controller from '@ember/controller';
import { tracked } from '@glimmer/tracking';
import { action } from '@ember/object';
import { service } from '@ember/service';
import { registerDestructor } from '@ember/destroyable';

export default class SearchController extends Controller {
  @service api;
  @service socket;
  @service router;

  constructor(owner, args) {
    super(owner, args);
    this.socket.on('item_added', this.#onItemAdded);
    this.socket.on('item_removed', this.#onItemRemoved);
    registerDestructor(this, () => {
      this.socket.off('item_added', this.#onItemAdded);
      this.socket.off('item_removed', this.#onItemRemoved);
    });
  }

  @tracked query = '';
  @tracked type = 'tv';
  @tracked results = [];
  @tracked loading = false;
  @tracked searched = false;
  @tracked error = null;
  @tracked addedPids = new Set();
  @tracked addedSeries = new Set();
  @tracked successMessage = null;
  @tracked refreshing = false;
  @tracked selectedShow = null;
  @tracked episodes = [];
  @tracked episodesLoading = false;
  @tracked episodesError = null;
  @tracked selectedSeries = null;

  get series() {
    const map = new Map();
    for (const ep of this.episodes) {
      const key = ep.series ?? 'Specials';
      if (!map.has(key)) map.set(key, []);
      map.get(key).push(ep);
    }
    return [...map.entries()]
      .sort(([a], [b]) => {
        // Sort "Series N" numerically; push "Specials" to the end
        const numA = parseInt(a.match(/\d+/)?.[0] ?? '9999', 10);
        const numB = parseInt(b.match(/\d+/)?.[0] ?? '9999', 10);
        return numA - numB;
      })
      .map(([name, episodes]) => ({ name, episodes }));
  }

  // BBC PID: one letter (b/p/m) followed by 7 lowercase alphanumerics, as a URL path segment or bare
  #pidRe = /(?:\/|^)([bpm][0-9a-z]{7})(?:[/?#]|$)/;

  /** If input is a BBC URL or a bare PID, return just the PID; otherwise return input unchanged. */
  #normalizeQuery(input) {
    const trimmed = input.trim();
    const m = this.#pidRe.exec(trimmed);
    return m ? m[1] : trimmed;
  }

  @action
  updateQuery(event) {
    this.query = event.target.value;
  }

  @action
  setType(t) {
    this.type = t;
  }

  @action
  async search(event) {
    event?.preventDefault();
    if (!this.query.trim()) return;

    const effectiveQuery = this.#normalizeQuery(this.query);

    // If we extracted a PID from a URL, update the field so the user can see what was used
    if (effectiveQuery !== this.query.trim()) {
      this.query = effectiveQuery;
    }

    this.loading = true;
    this.error = null;
    this.results = [];
    this.selectedShow = null;
    this.episodes = [];

    try {
      this.results = await this.api.search(effectiveQuery, this.type);
      this.searched = true;
    } catch (e) {
      this.error = e.message ?? 'Search failed';
    } finally {
      this.loading = false;
    }
  }

  @action
  async browseShow(result) {
    // Toggle: clicking the open show closes it
    if (this.selectedShow?.pid === result.pid) {
      this.selectedShow = null;
      this.episodes = [];
      this.selectedSeries = null;
      return;
    }
    this.selectedShow = result;
    this.episodes = [];
    this.selectedSeries = null;
    this.episodesLoading = true;
    this.episodesError = null;
    try {
      this.episodes = await this.api.fetchEpisodes(result.pid, this.type);
    } catch (e) {
      this.episodesError = e.message ?? 'Failed to load episodes';
    } finally {
      this.episodesLoading = false;
    }
  }

  @action
  closeEpisodes() {
    this.selectedShow = null;
    this.episodes = [];
    this.episodesError = null;
    this.selectedSeries = null;
  }

  @action
  browseSeries(name) {
    this.selectedSeries = this.selectedSeries === name ? null : name;
  }

  @action
  async queueSeries(series) {
    const unqueued = series.episodes.filter((ep) => !this.addedPids.has(ep.pid));
    for (const ep of unqueued) {
      // eslint-disable-next-line no-await-in-loop
      await this.addToQueue(ep);
    }
    this.addedSeries = new Set([...this.addedSeries, series.name]);
  }

  @action
  async addToQueue(result) {
    try {
      await this.api.addToQueue({
        pid: result.pid,
        title: result.title,
        series: result.series,
        episode: result.episode,
        channel: result.channel,
        media_type: this.type,
        // Fall back to the parent show thumbnail when the episode has none
        thumbnail_url: result.thumbnail_url ?? this.selectedShow?.thumbnail_url ?? null,
      });
      this.addedPids = new Set([...this.addedPids, result.pid]);
      this.successMessage = `Added "${result.title}" to queue`;
      setTimeout(() => (this.successMessage = null), 3000);
    } catch (e) {
      this.error = e.message ?? 'Failed to add to queue';
    }
  }

  #onItemAdded = (event) => {
    if (event.item?.pid) {
      this.addedPids = new Set([...this.addedPids, event.item.pid]);
    }
  };

  #onItemRemoved = (event) => {
    // Re-enable the Queue button when an item is deleted from the queue
    const next = new Set(this.addedPids);
    // We only have the queue item id, not pid — look it up in addedPids by
    // checking if any episode/series pid maps to this id via the model data.
    // Simpler: fetch the queue again to rebuild the set accurately.
    void this.api
      .fetchQueue({ per_page: 1000 })
      .then((result) => {
        const stillQueued = new Set(result.data.map((i) => i.pid));
        this.addedPids = new Set([...next].filter((p) => stillQueued.has(p)));
      })
      .catch(() => {});
    void event; // suppress unused var warning
  };

  @action
  async refreshCache() {
    this.refreshing = true;
    this.error = null;
    try {
      await this.api.refreshCache(this.type);
      this.successMessage = 'Cache rebuild started — search again in a moment';
      setTimeout(() => (this.successMessage = null), 5000);
    } catch (e) {
      this.error = e.message;
    } finally {
      this.refreshing = false;
    }
  }

  @action
  logout() {
    this.api.logout();
    this.router.transitionTo('login');
  }
}

import Service, { service } from '@ember/service';
import { tracked } from '@glimmer/tracking';
import config from 'tapedeck-ui/config/environment';

/**
 * Maintains a persistent WebSocket connection to the Tapedeck backend.
 *
 * Components and routes can subscribe to events using `on(type, handler)` and
 * unsubscribe with `off(type, handler)`.
 *
 * Automatically reconnects with exponential back-off.
 */
export default class SocketService extends Service {
  @service api;

  @tracked connected = false;

  #ws = null;
  #listeners = new Map(); // eventType → Set<handler>
  #reconnectDelay = 1000;
  #reconnectTimer = null;
  #intentionalClose = false;

  // ── Connection management ──────────────────────────────────────────────────

  connect() {
    if (this.#ws) return;

    const base = config.APP.apiBase ?? '';
    const wsBase = base.replace(/^http/, 'ws');
    const url = `${wsBase}/ws?token=${encodeURIComponent(this.api.token ?? '')}`;

    this.#intentionalClose = false;
    this.#open(url);
  }

  disconnect() {
    this.#intentionalClose = true;
    clearTimeout(this.#reconnectTimer);
    if (this.#ws) {
      this.#ws.close();
      this.#ws = null;
    }
    this.connected = false;
  }

  #open(url) {
    const ws = new WebSocket(url);
    this.#ws = ws;

    ws.addEventListener('open', () => {
      this.connected = true;
      this.#reconnectDelay = 1000;
    });

    ws.addEventListener('close', () => {
      this.connected = false;
      this.#ws = null;
      if (!this.#intentionalClose) {
        this.#scheduleReconnect(url);
      }
    });

    ws.addEventListener('error', () => {
      // close event will fire next; let it handle reconnection
    });

    ws.addEventListener('message', (event) => {
      let parsed;
      try {
        parsed = JSON.parse(event.data);
      } catch {
        return;
      }
      this.#dispatch(parsed);
    });
  }

  #scheduleReconnect(url) {
    clearTimeout(this.#reconnectTimer);
    this.#reconnectTimer = setTimeout(() => {
      if (!this.#intentionalClose && this.api.isAuthenticated) {
        this.#open(url);
      }
    }, this.#reconnectDelay);
    // Exponential back-off, capped at 30 s
    this.#reconnectDelay = Math.min(this.#reconnectDelay * 2, 30_000);
  }

  // ── Event bus ──────────────────────────────────────────────────────────────

  on(type, handler) {
    if (!this.#listeners.has(type)) {
      this.#listeners.set(type, new Set());
    }
    this.#listeners.get(type).add(handler);
  }

  off(type, handler) {
    this.#listeners.get(type)?.delete(handler);
  }

  #dispatch(event) {
    const handlers = this.#listeners.get(event.type);
    handlers?.forEach((fn) => fn(event));
    // Also dispatch to '*' catch-all listeners
    this.#listeners.get('*')?.forEach((fn) => fn(event));
  }

  // ── Cleanup ────────────────────────────────────────────────────────────────

  willDestroy() {
    super.willDestroy();
    this.disconnect();
  }
}

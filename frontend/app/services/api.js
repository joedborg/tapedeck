import Service from '@ember/service';
import { tracked } from '@glimmer/tracking';
import config from 'tapedeck-ui/config/environment';

const TOKEN_KEY = 'tapedeck_token';
const USER_KEY = 'tapedeck_user';

/**
 * Centralized HTTP API client.
 *
 * Handles:
 *  - Token storage / retrieval (localStorage)
 *  - Attaching Authorization header to every request
 *  - 401 → clear session and redirect to /login
 *  - JSON serialisation / deserialisation
 */
export default class ApiService extends Service {
  @tracked token = null;
  @tracked currentUser = null;

  get isAuthenticated() {
    return !!this.token;
  }

  get base() {
    return config.APP.apiBase ?? '';
  }

  // ── Session persistence ───────────────────────────────────────────────────

  restore() {
    this.token = localStorage.getItem(TOKEN_KEY);
    const raw = localStorage.getItem(USER_KEY);
    if (raw) {
      try {
        this.currentUser = JSON.parse(raw);
      } catch {
        this.currentUser = null;
      }
    }
  }

  persistSession(token, user) {
    this.token = token;
    this.currentUser = user;
    localStorage.setItem(TOKEN_KEY, token);
    localStorage.setItem(USER_KEY, JSON.stringify(user));
  }

  clearSession() {
    this.token = null;
    this.currentUser = null;
    localStorage.removeItem(TOKEN_KEY);
    localStorage.removeItem(USER_KEY);
  }

  // ── Core fetch wrapper ────────────────────────────────────────────────────

  async request(method, path, body) {
    const url = `${this.base}/api${path}`;
    const headers = { 'Content-Type': 'application/json' };
    if (this.token) {
      headers['Authorization'] = `Bearer ${this.token}`;
    }

    const res = await fetch(url, {
      method,
      headers,
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });

    if (res.status === 401) {
      this.clearSession();
      // Let callers handle the redirect
      throw new UnauthenticatedError();
    }

    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: res.statusText }));
      throw new ApiError(res.status, err.error ?? res.statusText);
    }

    if (res.status === 204 || res.status === 202) return null;

    return res.json();
  }

  get(path) {
    return this.request('GET', path);
  }

  post(path, body) {
    return this.request('POST', path, body);
  }

  put(path, body) {
    return this.request('PUT', path, body);
  }

  patch(path, body) {
    return this.request('PATCH', path, body);
  }

  delete(path) {
    return this.request('DELETE', path);
  }

  // ── Auth ──────────────────────────────────────────────────────────────────

  async login(username, password) {
    const result = await this.request('POST', '/auth/login', {
      username,
      password,
    });
    this.persistSession(result.token, {
      id: result.user_id,
      username: result.username,
    });
    return result;
  }

  logout() {
    this.clearSession();
  }

  // ── Queue ─────────────────────────────────────────────────────────────────

  fetchQueue(params = {}) {
    const qs = new URLSearchParams(params).toString();
    return this.get(`/queue${qs ? `?${qs}` : ''}`);
  }

  addToQueue(payload) {
    return this.post('/queue', payload);
  }

  removeFromQueue(id) {
    return this.delete(`/queue/${id}`);
  }

  retryDownload(id) {
    return this.post(`/queue/${id}/retry`);
  }

  reorderQueue(entries) {
    return this.post('/queue/reorder', entries);
  }

  // ── Search ────────────────────────────────────────────────────────────────

  search(query, type = 'tv') {
    const qs = new URLSearchParams({ q: query, type }).toString();
    return this.get(`/search?${qs}`);
  }

  refreshCache(type = 'tv') {
    return this.post('/search/refresh', { type });
  }

  fetchEpisodes(pid, type = 'tv') {
    const qs = new URLSearchParams({ pid, type }).toString();
    return this.get(`/search/episodes?${qs}`);
  }

  // ── Settings ──────────────────────────────────────────────────────────────

  fetchSettings() {
    return this.get('/settings');
  }

  updateSetting(key, value) {
    return this.put(`/settings/${key}`, { value });
  }

  bulkUpdateSettings(map) {
    return this.patch('/settings', map);
  }

  // ── Users ─────────────────────────────────────────────────────────────────

  fetchUsers() {
    return this.get('/users');
  }

  createUser(username, password) {
    return this.post('/users', { username, password });
  }

  deleteUser(id) {
    return this.delete(`/users/${id}`);
  }

  changePassword(id, newPassword) {
    return this.put(`/users/${id}/password`, { new_password: newPassword });
  }
}

export class UnauthenticatedError extends Error {
  name = 'UnauthenticatedError';
}

export class ApiError extends Error {
  name = 'ApiError';
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

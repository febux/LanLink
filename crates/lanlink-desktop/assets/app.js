const state = {
  snapshot: null,
  snapshotError: null,
  directory: [],
  peerSort: 'ip',
};

const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => Array.from(document.querySelectorAll(sel));

function formatBytes(n) {
  if (n === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log10(n) / 3), units.length - 1);
  const v = n / Math.pow(1000, i);
  return `${v.toFixed(i === 0 ? 0 : 2)} ${units[i]}`;
}

function formatUptime(s) {
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ${m % 60}m`;
  const d = Math.floor(h / 24);
  return `${d}d ${h % 24}h`;
}

function formatLastSeen(s) {
  if (s === undefined || s === null) return '—';
  if (s === 0) return 'now';
  if (s < 60) return `${s}s ago`;
  return `${Math.floor(s / 60)}m ago`;
}

function badge(kind) {
  return `<span class="badge ${kind}">${kind}</span>`;
}

function showTab(name) {
  $$('.tab').forEach((t) => t.classList.toggle('active', t.id === name));
  $$('.nav-btn').forEach((b) => b.classList.toggle('active', b.dataset.tab === name));
}

function renderStatus(online, text) {
  const dot = $('#status-dot');
  const label = $('#status-text');
  dot.className = `dot ${online ? 'online' : 'offline'}`;
  label.textContent = text;
}

function renderDashboard() {
  const snap = state.snapshot;
  const errorEl = $('#dashboard-error');
  if (!snap) {
    $('#dashboard-empty').classList.remove('hidden');
    $('#dashboard-content').classList.add('hidden');
    if (errorEl) {
      errorEl.textContent = state.snapshotError || '';
    }
    return;
  }
  state.snapshotError = null;
  $('#dashboard-empty').classList.add('hidden');
  $('#dashboard-content').classList.remove('hidden');

  const s = snap.status;
  $('#net-id').textContent = s.network_id;
  $('#net-ip').textContent = s.assigned_ip;
  $('#net-tap').textContent = s.tap_name || '—';
  $('#net-bind').textContent = s.bind || '—';
  $('#net-uptime').textContent = formatUptime(s.uptime_seconds);

  $('#feat-gateway').textContent = s.gateway_enabled ? 'enabled' : 'disabled';
  $('#feat-firewall').textContent = s.firewall_enabled ? 'enabled' : 'disabled';
  $('#feat-relay').textContent = s.relay_addr || 'none';

  $('#traffic-tx').textContent = formatBytes(s.tx_bytes);
  $('#traffic-rx').textContent = formatBytes(s.rx_bytes);
}

function renderPeers() {
  const snap = state.snapshot;
  if (!snap) {
    $('#peers-empty').classList.remove('hidden');
    $('#peers-content').classList.add('hidden');
    return;
  }
  $('#peers-empty').classList.add('hidden');
  $('#peers-content').classList.remove('hidden');

  let peers = [...snap.peers];
  switch (state.peerSort) {
    case 'latency':
      peers.sort((a, b) => (a.latency_ms ?? Infinity) - (b.latency_ms ?? Infinity));
      break;
    case 'traffic':
      peers.sort((a, b) => b.tx_bytes + b.rx_bytes - (a.tx_bytes + a.rx_bytes));
      break;
    case 'ip':
    default:
      peers.sort((a, b) => String(a.virtual_ip).localeCompare(String(b.virtual_ip)));
      break;
  }

  $('#peers-body').innerHTML = peers
    .map((p) => {
      const ip = p.virtual_ip;
      const endpoint = p.endpoint || '—';
      return `
        <tr>
          <td>${ip}</td>
          <td>${p.latency_ms !== undefined && p.latency_ms !== null ? `${p.latency_ms} ms` : '—'}</td>
          <td>${badge(p.path)}</td>
          <td>${endpoint}</td>
          <td>${formatLastSeen(p.last_seen_secs_ago)}</td>
          <td>${formatBytes(p.tx_bytes)}</td>
          <td>${formatBytes(p.rx_bytes)}</td>
          <td>${p.is_gateway ? (p.shared_subnet || 'yes') : '—'}</td>
        </tr>
      `;
    })
    .join('');
}

function renderDirectory() {
  const dir = state.directory;
  if (dir.length === 0) {
    $('#directory-empty').textContent = 'No public networks found.';
    $('#directory-empty').classList.remove('hidden');
    $('#directory-table').classList.add('hidden');
    return;
  }
  $('#directory-empty').classList.add('hidden');
  $('#directory-table').classList.remove('hidden');

  $('#directory-body').innerHTML = dir
    .map((n) => {
      return `
        <tr>
          <td>${escapeHtml(n.name)}</td>
          <td>${n.network_id}</td>
          <td>${n.subnet}</td>
          <td>${n.member_count}</td>
          <td>${badge(n.is_public ? 'public' : 'private')}</td>
          <td><button class="btn small" data-id="${n.network_id}">Join</button></td>
        </tr>
      `;
    })
    .join('');

  $$('#directory-body .btn').forEach((btn) => {
    btn.addEventListener('click', () => {
      showTab('networks');
      $('#join-id').value = btn.dataset.id;
      $('#join-password').focus();
    });
  });
}

function escapeHtml(s) {
  return s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

function toast(msg, isError = false) {
  const el = $('#toast');
  el.textContent = msg;
  el.className = `toast ${isError ? 'error' : ''} show`;
  setTimeout(() => el.classList.remove('show'), 4000);
}

function showResult(id, msg, isError) {
  const el = $(id);
  el.textContent = msg;
  el.className = `result ${isError ? 'error' : 'success'}`;
}

// Tauri invoke bridge: in a Tauri window this uses the secure command channel,
// in a normal browser it falls back to fetch so the same SPA can still be served
// by `lanlink-web` during development.
async function invoke(cmd, args = {}) {
  if (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke) {
    return await window.__TAURI__.core.invoke(cmd, args);
  }
  // Fallback for browser dev mode: assume a local lanlink-web server.
  const map = {
    get_snapshot: '/api/snapshot',
    list_directory: '/api/directory',
    create_network: '/api/networks',
    join_network: (a) => `/api/networks/${a.network_id}/join`,
  };
  let path = typeof map[cmd] === 'function' ? map[cmd](args) : map[cmd];
  const opts = { headers: { 'Content-Type': 'application/json' } };
  if (cmd === 'create_network') {
    opts.method = 'POST';
    opts.body = JSON.stringify({ name: args.name, password: args.password, is_public: args.is_public });
  } else if (cmd === 'join_network') {
    opts.method = 'POST';
    opts.body = JSON.stringify({ password: args.password, shared_subnet: args.shared_subnet || '' });
  }
  const res = await fetch(path, opts);
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`);
  if (cmd === 'get_snapshot') return data.snapshot || null;
  if (cmd === 'list_directory') return data.networks || [];
  return data.result || data;
}

async function refreshSnapshot() {
  try {
    const snapshot = await invoke('get_snapshot');
    if (snapshot) {
      state.snapshot = snapshot;
      state.snapshotError = null;
      renderStatus(true, 'daemon online');
    } else {
      state.snapshot = null;
      state.snapshotError = 'daemon returned empty snapshot';
      renderStatus(false, state.snapshotError);
    }
  } catch (e) {
    state.snapshot = null;
    const msg = e?.message || String(e) || 'daemon offline';
    state.snapshotError = msg;
    renderStatus(false, msg);
  }
  renderDashboard();
  renderPeers();
}

async function refreshDirectory() {
  try {
    state.directory = await invoke('list_directory');
  } catch (e) {
    state.directory = [];
    console.warn('directory refresh failed:', e);
  }
  renderDirectory();
}

async function submitCreate(e) {
  e.preventDefault();
  const btn = $('#create-form button[type="submit"]');
  btn.disabled = true;
  try {
    const r = await invoke('create_network', {
      name: $('#create-name').value.trim(),
      password: $('#create-password').value,
      is_public: $('#create-public').checked,
    });
    showResult(
      '#create-result',
      `Created network "${escapeHtml($('#create-name').value.trim())}" — id ${r.network_id}, subnet ${r.subnet}`,
      false
    );
    $('#create-form').reset();
    refreshDirectory();
  } catch (e) {
    showResult('#create-result', e.message, true);
  } finally {
    btn.disabled = false;
  }
}

async function submitJoin(e) {
  e.preventDefault();
  const btn = $('#join-form button[type="submit"]');
  btn.disabled = true;
  try {
    const r = await invoke('join_network', {
      network_id: Number($('#join-id').value),
      password: $('#join-password').value,
      shared_subnet: '',
    });
    showResult(
      '#join-result',
      `Joined network ${r.network_id} — assigned IP ${r.assigned_ip}`,
      false
    );
    $('#join-form').reset();
  } catch (e) {
    showResult('#join-result', e.message, true);
  } finally {
    btn.disabled = false;
  }
}

async function refreshAboutSocket() {
  try {
    const path = await invoke('resolve_status_socket');
    const el = $('#about-socket');
    if (el) {
      el.textContent = path || 'none found in /run/lanlink or /tmp';
    }
  } catch (e) {
    const el = $('#about-socket');
    if (el) {
      el.textContent = `error: ${e?.message || String(e)}`;
    }
  }
}

function init() {
  console.log('lanlink-desktop UI initializing; Tauri available:', !!window.__TAURI__);
  $$('.nav-btn').forEach((btn) => {
    btn.addEventListener('click', () => showTab(btn.dataset.tab));
  });

  $('#peer-sort').addEventListener('change', (e) => {
    state.peerSort = e.target.value;
    renderPeers();
  });

  $('#create-form').addEventListener('submit', submitCreate);
  $('#join-form').addEventListener('submit', submitJoin);

  refreshSnapshot();
  refreshDirectory();
  refreshAboutSocket();

  setInterval(refreshSnapshot, 1000);
  setInterval(refreshDirectory, 10000);
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', init);
} else {
  init();
}

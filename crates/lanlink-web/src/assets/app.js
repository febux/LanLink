const state = {
  snapshot: null,
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
  if (!snap) {
    $('#dashboard-empty').classList.remove('hidden');
    $('#dashboard-content').classList.add('hidden');
    return;
  }
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

async function api(path, opts = {}) {
  const res = await fetch(path, {
    headers: { 'Content-Type': 'application/json' },
    ...opts,
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`);
  return data;
}

async function refreshSnapshot() {
  try {
    const res = await fetch('/api/snapshot', {
      headers: { 'Content-Type': 'application/json' },
    });
    const data = await res.json().catch(() => ({}));
    if (res.ok && data.snapshot) {
      state.snapshot = data.snapshot;
      renderStatus(true, 'daemon online');
    } else {
      state.snapshot = null;
      const msg = data.error || (res.status === 503 ? 'daemon unavailable' : `HTTP ${res.status}`);
      renderStatus(false, msg);
    }
  } catch (e) {
    state.snapshot = null;
    renderStatus(false, e.message || 'daemon offline');
  }
  renderDashboard();
  renderPeers();
}

async function refreshDirectory() {
  try {
    const data = await api('/api/directory');
    state.directory = data.networks || [];
  } catch (e) {
    state.directory = [];
  }
  renderDirectory();
}

async function submitCreate(e) {
  e.preventDefault();
  const btn = $('#create-form button[type="submit"]');
  btn.disabled = true;
  try {
    const data = await api('/api/networks', {
      method: 'POST',
      body: JSON.stringify({
        name: $('#create-name').value.trim(),
        password: $('#create-password').value,
        is_public: $('#create-public').checked,
      }),
    });
    const r = data.result;
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
    const data = await api(`/api/networks/${$('#join-id').value}/join`, {
      method: 'POST',
      body: JSON.stringify({
        password: $('#join-password').value,
        shared_subnet: '',
      }),
    });
    const r = data.result;
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

function init() {
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

  setInterval(refreshSnapshot, 1000);
  setInterval(refreshDirectory, 10000);
}

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', init);
} else {
  init();
}

const state = {
  snapshot: null,
  snapshotError: null,
  directory: [],
  peerSort: 'ip',
  isWindows: navigator.userAgent.includes('Windows') || false,
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
  const servicePanel = $('#windows-service-panel');
  if (!snap) {
    $('#dashboard-empty').classList.remove('hidden');
    $('#dashboard-content').classList.add('hidden');
    if (errorEl) {
      errorEl.textContent = state.snapshotError || '';
    }
    if (servicePanel) {
      servicePanel.classList.toggle('hidden', !state.isWindows);
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
  if (map[cmd] === undefined) {
    throw new Error(`${cmd} is only available in the Tauri desktop app`);
  }
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
  logServiceState().catch(() => {});
  refreshCoordinatorState().catch(() => {});
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
      isPublic: $('#create-public').checked,
    });
    showResult(
      '#create-result',
      `Created network "${escapeHtml($('#create-name').value.trim())}" — id ${r.network_id}, subnet ${r.subnet}`,
      false
    );
    $('#create-form').reset();
    refreshDirectory();
  } catch (e) {
    showResult('#create-result', e?.message || String(e), true);
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
      networkId: Number($('#join-id').value),
      password: $('#join-password').value,
      sharedSubnet: '',
    });
    showResult(
      '#join-result',
      `Joined network ${r.network_id} — assigned IP ${r.assigned_ip}`,
      false
    );
    $('#join-form').reset();
  } catch (e) {
    showResult('#join-result', e?.message || String(e), true);
  } finally {
    btn.disabled = false;
  }
}

async function refreshAboutSocket() {
  try {
    const path = await invoke('resolve_status_socket');
    const el = $('#about-socket');
    if (el) {
      el.textContent = path || (state.isWindows ? 'none found on TCP loopback' : 'none found in /run/lanlink or /tmp');
    }
  } catch (e) {
    const el = $('#about-socket');
    if (el) {
      el.textContent = `error: ${e?.message || String(e)}`;
    }
  }
}

function logService(msg) {
  const el = $('#service-log');
  if (el) {
    el.textContent += (el.textContent ? '\n' : '') + msg;
    el.scrollTop = el.scrollHeight;
  }
}

async function runServiceCommand(cmd) {
  try {
    const result = await invoke(cmd);
    logService(`${cmd}: ${result}`);
    toast(`${cmd} succeeded`);
  } catch (e) {
    const msg = e?.message || String(e);
    logService(`${cmd} failed: ${msg}`);
    toast(`${cmd} failed: ${msg}`, true);
  }
}

async function dashboardAction(cmd, label) {
  try {
    const result = await invoke(cmd);
    toast(`${label}: ${result}`);
    refreshSnapshot();
  } catch (e) {
    const msg = e?.message || String(e);
    toast(`${label} failed: ${msg}`, true);
  }
}

async function submitServiceUp(e) {
  e.preventDefault();
  const btn = $('#service-up-form button[type="submit"]');
  btn.disabled = true;
  try {
    const result = await invoke('service_up', {
      name: $('#up-name').value.trim(),
      password: $('#up-password').value,
      coordinator: $('#up-coordinator').value.trim() || undefined,
      public: $('#up-public').checked,
    });
    showResult('#service-up-result', result, false);
    $('#service-up-form').reset();
    setTimeout(refreshSnapshot, 2000);
    setTimeout(logServiceState, 3000);
  } catch (e) {
    const msg = e?.message || String(e);
    showResult(
      '#service-up-result',
      msg +
        '\n\nIf the file/secret could not be created, try this in an elevated PowerShell:\n' +
        `  lanlinkd service up --name "${$('#up-name').value.trim()}" --password "..."`,
      true
    );
  } finally {
    btn.disabled = false;
  }
}

async function logServiceState() {
  // Live, read-only service diagnosis for the empty dashboard (Windows).
  // `service_status` is a plain SCM query: no UAC prompt. Throttled so the
  // 1s snapshot poll does not spam SCM queries.
  const now = Date.now();
  if (!state.isWindows || state.snapshot) return;
  if (!window.__TAURI__) return; // browser dev mode has no service bridge
  if (now - (logServiceState.lastAt || 0) < 5000) return;
  logServiceState.lastAt = now;
  const el = $('#service-state-line');
  if (!el) return;
  let text;
  try {
    const out = await invoke('service_status');
    const trimmed = String(out).trim();
    if (/running/i.test(trimmed)) {
      let cfgExists = null;
      try {
        cfgExists = await invoke('daemon_config_exists');
      } catch (_) { /* non-Windows or older backend */ }
      text = cfgExists === false
        ? 'Service: running, but no daemon config exists and the status endpoint is unreachable. Create or join a network below \u2014 it writes the config and starts the daemon. (You also need a coordinator \u2014 use \u201CRun built-in coordinator\u201D above.)'
        : 'Service: running, but the status endpoint is unreachable. Check the [status] socket in C:\\Program Files\\LanLink\\lanlinkd.toml (port = 9000 + network id), then stop and start the service.';
    } else if (/startpending/i.test(trimmed)) {
      text = 'Service: starting\u2026';
    } else if (/stoppending/i.test(trimmed)) {
      text = 'Service: stopping\u2026';
    } else if (/stopped/i.test(trimmed)) {
      let logTail = '';
      try {
        const tail = await invoke('daemon_log_tail');
        if (tail) logTail = '\n' + tail;
      } catch (_) {}
      text = /exit code/i.test(trimmed)
        ? `Service: ${trimmed}${logTail}\n\nCheck C:\\Program Files\\LanLink\\lanlinkd.log for the full startup error. If the log mentions "No driver found", "create TAP device", or "Wintun", install the OpenVPN TAP-Windows6 driver or Wintun, then stop and start the service.`
        : 'Service: stopped. Create or join a network below (recommended) \u2014 it configures and starts the service. Or press \u201CStart service\u201D if a network is already configured.';
    } else {
      text = `Service: ${trimmed}`;
    }
  } catch (e) {
    const msg = e?.message || String(e) || '';
    if (/failed to open service|1060|does not exist/i.test(msg)) {
      text = 'Service: not installed yet. Create or join a network below \u2014 it installs and starts the service automatically (UAC prompt).';
    } else if (msg) {
      text = `Service check failed: ${msg}`;
    } else {
      text = 'Service: unknown state.';
    }
  }
  if (el.textContent !== text) el.textContent = text;
}

async function refreshCoordinatorState() {
  const el = $('#coord-state');
  if (!el || !window.__TAURI__) return;
  if (!state.isWindows) return;
  const now = Date.now();
  if (now - (refreshCoordinatorState.lastAt || 0) < 5000) return;
  refreshCoordinatorState.lastAt = now;
  try {
    const up = await invoke('coordinator_status');
    el.textContent = up
      ? 'Coordinator: reachable.'
      : 'Coordinator: unreachable \u2014 create/join needs one. Run the built-in coordinator, or point the form at a remote one.';
    el.classList.toggle('coord-bad', !up);
  } catch (e) {
    el.textContent = `Coordinator check failed: ${e?.message || String(e)}`;
    el.classList.add('coord-bad');
  }
}

async function coordinatorCommand(cmd) {
  const el = $('#coord-state');
  try {
    const result = await invoke(cmd);
    toast(`${cmd === 'coordinator_start' ? 'Coordinator started' : 'Coordinator stopped'}`);
    if (el) el.textContent = `Coordinator: ${result}`;
    refreshCoordinatorState.lastAt = 0;
  } catch (e) {
    const msg = e?.message || String(e);
    toast(`coordinator ${cmd} failed: ${msg}`, true);
    if (el) el.textContent = `Coordinator: ${msg}`;
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

  if ($('#service-up-form')) {
    $('#service-up-form').addEventListener('submit', submitServiceUp);
  }
  if ($('#svc-install')) $('#svc-install').addEventListener('click', () => runServiceCommand('service_install'));
  if ($('#svc-start')) $('#svc-start').addEventListener('click', () => runServiceCommand('service_start'));
  if ($('#svc-stop')) $('#svc-stop').addEventListener('click', () => runServiceCommand('service_stop'));
  if ($('#svc-status')) $('#svc-status').addEventListener('click', () => runServiceCommand('service_status'));
  if ($('#btn-disconnect')) {
    $('#btn-disconnect').addEventListener('click', async () => {
      const btn = $('#btn-disconnect');
      btn.disabled = true;
      try {
        await dashboardAction('disconnect_network', 'Disconnect');
      } finally {
        btn.disabled = false;
      }
    });
  }
  if ($('#btn-stop')) {
    $('#btn-stop').addEventListener('click', async () => {
      const btn = $('#btn-stop');
      btn.disabled = true;
      try {
        await dashboardAction('stop_daemon', 'Stop daemon');
      } finally {
        btn.disabled = false;
      }
    });
  }
  if ($('#coord-start')) $('#coord-start').addEventListener('click', () => coordinatorCommand('coordinator_start'));
  if ($('#coord-stop')) $('#coord-stop').addEventListener('click', () => coordinatorCommand('coordinator_stop'));

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

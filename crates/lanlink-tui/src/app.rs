//! Application state and the event reducer. Kept free of terminal and
//! network concerns so it can be unit-tested headlessly: `on_event` maps an
//! [`Event`] (plus optional pending [`Action`]s) to state changes only.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyModifiers};
use lanlink_proto::status::StatusSnapshot;
use lanlink_proto::{JoinNetworkResponse, PublicNetworkInfo};

use crate::coord::Action;

/// Screen tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Dashboard,
    Peers,
    Networks,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Dashboard, Tab::Peers, Tab::Networks];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Dashboard => "Dashboard",
            Tab::Peers => "Peers",
            Tab::Networks => "Networks",
        }
    }

    fn index(self) -> usize {
        match self {
            Tab::Dashboard => 0,
            Tab::Peers => 1,
            Tab::Networks => 2,
        }
    }

    fn from_index(i: usize) -> Self {
        Self::ALL[i % Self::ALL.len()]
    }

    fn next(self) -> Self {
        Self::from_index(self.index() + 1)
    }

    fn prev(self) -> Self {
        Self::from_index(self.index() + Self::ALL.len() - 1)
    }
}

/// Where keyboard input currently goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Tabs,
    DirList,
    CreateForm,
    JoinForm,
    Help,
}

/// Peers-table sort order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerSort {
    Latency,
    Ip,
    Traffic,
}

impl PeerSort {
    pub fn title(self) -> &'static str {
        match self {
            PeerSort::Latency => "latency",
            PeerSort::Ip => "ip",
            PeerSort::Traffic => "traffic",
        }
    }

    pub fn next(self) -> Self {
        match self {
            PeerSort::Latency => PeerSort::Ip,
            PeerSort::Ip => PeerSort::Traffic,
            PeerSort::Traffic => PeerSort::Latency,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Error,
}

/// A transient footer message.
pub struct Toast {
    pub message: String,
    pub severity: Severity,
    pub at: Instant,
    pub ttl: Duration,
}

/// A simple multi-field text form with one optional boolean toggle and one
/// optional password field.
pub struct Form {
    pub title: &'static str,
    pub labels: &'static [&'static str],
    pub values: Vec<String>,
    pub cursor: usize,
    /// Index of the boolean toggle field, if any.
    pub toggle: Option<usize>,
    /// Index of the password field, if any (rendered masked).
    pub password: Option<usize>,
}

impl Form {
    pub fn new(
        title: &'static str,
        labels: &'static [&'static str],
        toggle: Option<usize>,
        password: Option<usize>,
    ) -> Self {
        Self {
            title,
            labels,
            values: vec![String::new(); labels.len()],
            cursor: 0,
            toggle,
            password,
        }
    }

    pub fn set(&mut self, field: usize, value: &str) {
        if let Some(v) = self.values.get_mut(field) {
            *v = value.to_string();
        }
    }

    pub fn value(&self, field: usize) -> &str {
        self.values.get(field).map(String::as_str).unwrap_or("")
    }

    pub fn is_toggled(&self) -> bool {
        self.toggle
            .and_then(|i| self.values.get(i))
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    fn input(&mut self, c: char) {
        if self.cursor == self.toggle.unwrap_or(usize::MAX) {
            return; // boolean field is edited with space, not text
        }
        if let Some(v) = self.values.get_mut(self.cursor) {
            v.push(c);
        }
    }

    fn backspace(&mut self) {
        if self.cursor == self.toggle.unwrap_or(usize::MAX) {
            return;
        }
        if let Some(v) = self.values.get_mut(self.cursor) {
            v.pop();
        }
    }

    fn advance(&mut self) {
        self.cursor = (self.cursor + 1) % self.labels.len();
    }

    fn toggle_bool(&mut self) {
        if let Some(i) = self.toggle {
            if self.cursor == i {
                let v = &mut self.values[i];
                *v = if *v == "1" { "0".into() } else { "1".into() };
            }
        }
    }

    fn reset(&mut self) {
        for v in &mut self.values {
            v.clear();
        }
        self.cursor = 0;
    }
}

/// Events flowing into the reducer from tasks and the input thread.
pub enum Event {
    Key(crossterm::event::KeyEvent),
    Tick,
    Snapshot(Result<StatusSnapshot, String>),
    Directory(Result<Vec<PublicNetworkInfo>, String>),
    Coord(Action, Result<crate::coord::CoordOutcome, String>),
}

/// The whole application state.
pub struct App {
    pub tab: Tab,
    pub focus: Focus,
    pub coordinator: String,
    pub socket_desc: String,

    pub snapshot: Option<StatusSnapshot>,
    pub snapshot_error: Option<String>,
    pub directory: Vec<PublicNetworkInfo>,
    pub directory_error: Option<String>,
    pub dir_selected: usize,

    pub peer_sort: PeerSort,

    pub create_form: Form,
    pub join_form: Form,
    pub join_result: Option<JoinNetworkResponse>,

    /// An action is currently in flight (suppresses duplicate submissions).
    pub busy: bool,
    pub toast: Option<Toast>,
    pub quit: bool,

    /// Bytes-per-second history for the dashboard sparklines.
    pub tx_rate: Vec<u64>,
    pub rx_rate: Vec<u64>,
    last_snapshot_at: Option<Instant>,
    last_tx_bytes: Option<u64>,
    last_rx_bytes: Option<u64>,
}

impl App {
    pub fn new(coordinator: String, socket_desc: String) -> Self {
        Self {
            tab: Tab::Dashboard,
            focus: Focus::Tabs,
            coordinator,
            socket_desc,
            snapshot: None,
            snapshot_error: None,
            directory: Vec::new(),
            directory_error: None,
            dir_selected: 0,
            peer_sort: PeerSort::Latency,
            create_form: Form::new(
                "Create network",
                &["Name", "Password", "Public (gaming firewall)"],
                Some(2),
                Some(1),
            ),
            join_form: Form::new("Join network", &["Network ID", "Password"], None, Some(1)),
            join_result: None,
            busy: false,
            toast: None,
            quit: false,
            tx_rate: Vec::new(),
            rx_rate: Vec::new(),
            last_snapshot_at: None,
            last_tx_bytes: None,
            last_rx_bytes: None,
        }
    }

    /// Feed one event; returns actions for the caller to dispatch.
    pub fn on_event(&mut self, event: Event) -> Vec<Action> {
        match event {
            Event::Key(key) => self.on_key(key),
            Event::Tick => {
                self.expire_toast();
                Vec::new()
            }
            Event::Snapshot(Ok(snapshot)) => {
                self.absorb_snapshot(snapshot);
                Vec::new()
            }
            Event::Snapshot(Err(error)) => {
                self.snapshot = None;
                self.snapshot_error = Some(error);
                Vec::new()
            }
            Event::Directory(Ok(dir)) => {
                self.directory = dir;
                self.directory_error = None;
                if self.dir_selected >= self.directory.len() {
                    self.dir_selected = self.directory.len().saturating_sub(1);
                }
                Vec::new()
            }
            Event::Directory(Err(error)) => {
                self.directory_error = Some(error);
                Vec::new()
            }
            Event::Coord(action, result) => self.absorb_coord(action, result),
        }
    }

    fn on_key(&mut self, key: crossterm::event::KeyEvent) -> Vec<Action> {
        use crossterm::event::KeyEventKind;
        if key.kind != KeyEventKind::Press {
            return Vec::new();
        }

        // Ctrl-C always quits, even while typing.
        if matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return Vec::new();
        }

        match self.focus {
            Focus::Help => {
                self.focus = Focus::Tabs;
            }
            Focus::CreateForm | Focus::JoinForm => return self.on_form_key(key),
            Focus::DirList => return self.on_dirlist_key(key),
            Focus::Tabs => return self.on_tabs_key(key),
        }
        Vec::new()
    }

    fn on_tabs_key(&mut self, key: crossterm::event::KeyEvent) -> Vec<Action> {
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.focus = Focus::Help,
            KeyCode::Tab | KeyCode::Right => self.tab = self.tab.next(),
            KeyCode::BackTab | KeyCode::Left => self.tab = self.tab.prev(),
            KeyCode::Char('1') => self.tab = Tab::Dashboard,
            KeyCode::Char('2') => self.tab = Tab::Peers,
            KeyCode::Char('3') => self.tab = Tab::Networks,
            KeyCode::Char('s') if self.tab == Tab::Peers => {
                self.peer_sort = self.peer_sort.next();
            }
            KeyCode::Down | KeyCode::Up if self.tab == Tab::Networks => {
                self.focus = Focus::DirList;
            }
            KeyCode::Char('c') if self.tab == Tab::Networks => {
                self.create_form.reset();
                self.focus = Focus::CreateForm;
            }
            KeyCode::Char('j') if self.tab == Tab::Networks => {
                self.join_form.reset();
                self.focus = Focus::JoinForm;
            }
            KeyCode::Enter if self.tab == Tab::Networks => {
                // Pre-fill the join form with the selected network id.
                if let Some(net) = self.directory.get(self.dir_selected) {
                    self.join_form.reset();
                    self.join_form.set(0, &net.network_id.0.to_string());
                    self.focus = Focus::JoinForm;
                } else {
                    self.show_toast("no network selected", Severity::Info);
                }
            }
            _ => {}
        }
        Vec::new()
    }

    fn on_dirlist_key(&mut self, key: crossterm::event::KeyEvent) -> Vec<Action> {
        match key.code {
            KeyCode::Esc | KeyCode::Tab => self.focus = Focus::Tabs,
            KeyCode::Up => {
                self.dir_selected = self.dir_selected.saturating_sub(1);
            }
            KeyCode::Down => {
                if !self.directory.is_empty() {
                    self.dir_selected = (self.dir_selected + 1).min(self.directory.len() - 1);
                }
            }
            KeyCode::Enter => {
                if let Some(net) = self.directory.get(self.dir_selected) {
                    self.join_form.reset();
                    self.join_form.set(0, &net.network_id.0.to_string());
                    self.focus = Focus::JoinForm;
                }
            }
            KeyCode::Char('c') => {
                self.create_form.reset();
                self.focus = Focus::CreateForm;
            }
            KeyCode::Char('j') => {
                self.join_form.reset();
                self.focus = Focus::JoinForm;
            }
            _ => return Vec::new(),
        }
        Vec::new()
    }

    fn on_form_key(&mut self, key: crossterm::event::KeyEvent) -> Vec<Action> {
        let creating = self.focus == Focus::CreateForm;
        let form = if creating {
            &mut self.create_form
        } else {
            &mut self.join_form
        };

        match key.code {
            KeyCode::Esc => {
                self.focus = Focus::Tabs;
            }
            KeyCode::Backspace => form.backspace(),
            KeyCode::Tab => form.advance(),
            KeyCode::Char(' ') => {
                if form.cursor == form.toggle.unwrap_or(usize::MAX) {
                    form.toggle_bool();
                } else {
                    form.input(' ');
                }
            }
            KeyCode::Enter => {
                if form.cursor + 1 < form.labels.len() {
                    form.advance();
                } else {
                    return self.submit_form();
                }
            }
            KeyCode::Char(c) => form.input(c),
            _ => {}
        }
        Vec::new()
    }

    fn submit_form(&mut self) -> Vec<Action> {
        if self.busy {
            self.show_toast("another operation is in flight", Severity::Info);
            return Vec::new();
        }

        if self.focus == Focus::CreateForm {
            let name = self.create_form.value(0).trim().to_string();
            if name.is_empty() {
                self.show_toast("network name is required", Severity::Error);
                return Vec::new();
            }
            let password = self.create_form.value(1).to_string();
            if password.is_empty() {
                self.show_toast("password is required", Severity::Error);
                return Vec::new();
            }
            self.busy = true;
            self.focus = Focus::Tabs;
            vec![Action::CreateNetwork {
                name,
                password,
                public: self.create_form.is_toggled(),
            }]
        } else {
            let id_text = self.join_form.value(0).trim().to_string();
            let Ok(network_id) = id_text.parse::<u64>() else {
                self.show_toast("network id must be a number", Severity::Error);
                return Vec::new();
            };
            let password = self.join_form.value(1).to_string();
            if password.is_empty() {
                self.show_toast("password is required", Severity::Error);
                return Vec::new();
            }
            self.busy = true;
            self.focus = Focus::Tabs;
            vec![Action::JoinNetwork {
                network_id,
                password,
            }]
        }
    }

    fn absorb_coord(
        &mut self,
        action: Action,
        result: Result<crate::coord::CoordOutcome, String>,
    ) -> Vec<Action> {
        self.busy = false;
        match (&action, result) {
            (Action::CreateNetwork { name, .. }, Ok(crate::coord::CoordOutcome::Created(resp))) => {
                self.show_toast(
                    format!(
                        "created \"{}\" (id {}, subnet {})",
                        name, resp.network_id.0, resp.subnet
                    ),
                    Severity::Info,
                );
            }
            (
                Action::JoinNetwork { network_id, .. },
                Ok(crate::coord::CoordOutcome::Joined(resp)),
            ) => {
                self.join_result = Some(resp.clone());
                self.show_toast(
                    format!("joined network {network_id}: see the Networks tab"),
                    Severity::Info,
                );
            }
            (_, Err(error)) => {
                self.show_toast(format!("coordinator: {error}"), Severity::Error);
            }
            (action, Ok(_)) => {
                self.show_toast(format!("unexpected result for {action:?}"), Severity::Error);
            }
        }
        Vec::new()
    }

    fn absorb_snapshot(&mut self, snapshot: StatusSnapshot) {
        let now = Instant::now();
        if let (Some(prev_at), Some(prev_tx), Some(prev_rx)) = (
            self.last_snapshot_at,
            self.last_tx_bytes,
            self.last_rx_bytes,
        ) {
            let dt = now.duration_since(prev_at).as_secs_f64().max(0.05);
            let tx = snapshot.status.tx_bytes.saturating_sub(prev_tx);
            let rx = snapshot.status.rx_bytes.saturating_sub(prev_rx);
            push_rate(&mut self.tx_rate, (tx as f64 / dt) as u64);
            push_rate(&mut self.rx_rate, (rx as f64 / dt) as u64);
        }
        self.last_snapshot_at = Some(now);
        self.last_tx_bytes = Some(snapshot.status.tx_bytes);
        self.last_rx_bytes = Some(snapshot.status.rx_bytes);
        self.snapshot_error = None;
        self.snapshot = Some(snapshot);
    }

    fn expire_toast(&mut self) {
        if let Some(toast) = &self.toast {
            if toast.at.elapsed() > toast.ttl {
                self.toast = None;
            }
        }
    }

    pub fn show_toast(&mut self, message: impl Into<String>, severity: Severity) {
        self.toast = Some(Toast {
            message: message.into(),
            severity,
            at: Instant::now(),
            ttl: Duration::from_secs(5),
        });
    }

    /// Peers sorted for the table according to the current sort mode.
    pub fn sorted_peers(&self) -> Vec<&lanlink_proto::status::PeerStatus> {
        let Some(snapshot) = &self.snapshot else {
            return Vec::new();
        };
        let mut peers: Vec<_> = snapshot.peers.iter().collect();
        match self.peer_sort {
            PeerSort::Latency => peers.sort_by_key(|p| p.latency_ms.unwrap_or(u64::MAX)),
            PeerSort::Ip => peers.sort_by_key(|p| p.virtual_ip.0.octets()),
            PeerSort::Traffic => peers.sort_by_key(|p| p.tx_bytes + p.rx_bytes),
        }
        peers
    }

    /// True when the daemon status socket is unreachable.
    pub fn daemon_offline(&self) -> bool {
        self.snapshot.is_none() && self.snapshot_error.is_some()
    }
}

/// Append to a bounded rate-history ring.
fn push_rate(history: &mut Vec<u64>, value: u64) {
    const MAX: usize = 60;
    history.push(value);
    if history.len() > MAX {
        history.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyEventKind, KeyModifiers};
    use lanlink_proto::status::{DaemonStatus, PathKind, PeerStatus};
    use lanlink_proto::{NetworkId, NodeId, VirtualIp};

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn key_with_mod(code: KeyCode, mods: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, mods))
    }

    fn make_app() -> App {
        App::new(
            "http://localhost:8080".into(),
            "/tmp/lanlinkd-1.sock".into(),
        )
    }

    fn sample_snapshot(tx: u64, rx: u64) -> StatusSnapshot {
        StatusSnapshot {
            status: DaemonStatus {
                version: "0.1.0".into(),
                network_id: NetworkId(1),
                assigned_ip: VirtualIp("26.0.0.1".parse().unwrap()),
                tap_name: "lanlink0".into(),
                bind: "0.0.0.0:9000".parse().unwrap(),
                relay_addr: None,
                gateway_enabled: false,
                firewall_enabled: false,
                uptime_seconds: 10,
                tx_bytes: tx,
                rx_bytes: rx,
            },
            peers: vec![PeerStatus {
                node_id: NodeId([0, 0, 0, 0, 0, 0, 0, 2]),
                virtual_ip: VirtualIp("26.0.0.2".parse().unwrap()),
                endpoint: Some("127.0.0.1:9100".parse().unwrap()),
                path: PathKind::Direct,
                latency_ms: Some(7),
                last_seen_secs_ago: Some(0),
                tx_bytes: 1,
                rx_bytes: 2,
                is_gateway: false,
                shared_subnet: String::new(),
            }],
        }
    }

    #[test]
    fn tabs_switch_with_keys_and_numbers() {
        let mut app = make_app();
        assert_eq!(app.tab, Tab::Dashboard);
        app.on_event(key(KeyCode::Char('2')));
        assert_eq!(app.tab, Tab::Peers);
        app.on_event(key(KeyCode::Right));
        assert_eq!(app.tab, Tab::Networks);
        app.on_event(key(KeyCode::Left));
        assert_eq!(app.tab, Tab::Peers);
        app.on_event(key(KeyCode::Char('1')));
        assert_eq!(app.tab, Tab::Dashboard);
    }

    #[test]
    fn q_and_ctrl_c_quit() {
        let mut app = make_app();
        app.on_event(key(KeyCode::Char('q')));
        assert!(app.quit);

        let mut app = make_app();
        app.on_event(key_with_mod(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.quit);
    }

    #[test]
    fn help_opens_and_closes() {
        let mut app = make_app();
        app.on_event(key(KeyCode::Char('?')));
        assert_eq!(app.focus, Focus::Help);
        app.on_event(key(KeyCode::Esc));
        assert_eq!(app.focus, Focus::Tabs);
    }

    #[test]
    fn peer_sort_cycles() {
        let mut app = make_app();
        app.on_event(key(KeyCode::Char('2')));
        assert_eq!(app.peer_sort, PeerSort::Latency);
        app.on_event(key(KeyCode::Char('s')));
        assert_eq!(app.peer_sort, PeerSort::Ip);
        app.on_event(key(KeyCode::Char('s')));
        assert_eq!(app.peer_sort, PeerSort::Traffic);
        app.on_event(key(KeyCode::Char('s')));
        assert_eq!(app.peer_sort, PeerSort::Latency);
    }

    #[test]
    fn create_form_types_toggles_and_submits() {
        let mut app = make_app();
        app.on_event(key(KeyCode::Char('3'))); // Networks tab
        app.on_event(key(KeyCode::Char('c'))); // open create form
        assert_eq!(app.focus, Focus::CreateForm);

        for c in "game night".chars() {
            app.on_event(key(KeyCode::Char(c)));
        }
        app.on_event(key(KeyCode::Tab)); // to password
        for c in "pw123".chars() {
            app.on_event(key(KeyCode::Char(c)));
        }
        app.on_event(key(KeyCode::Tab)); // to toggle
        app.on_event(key(KeyCode::Char(' '))); // toggle public on
        assert!(app.create_form.is_toggled());
        app.on_event(key(KeyCode::Backspace)); // backspace ignored on bool
        assert!(app.create_form.is_toggled());

        // One more Tab wraps to field 0; Enter then only advances, so submit
        // by navigating back to the last field first.
        app.on_event(key(KeyCode::Tab)); // wrap to name
        app.on_event(key(KeyCode::Tab));
        app.on_event(key(KeyCode::Tab)); // back to toggle (last field)
        let actions = app.on_event(key(KeyCode::Enter));
        assert_eq!(
            actions,
            vec![Action::CreateNetwork {
                name: "game night".into(),
                password: "pw123".into(),
                public: true,
            }]
        );
        assert!(app.busy);
        assert_eq!(app.focus, Focus::Tabs);
    }

    #[test]
    fn create_form_requires_name_and_password() {
        let mut app = make_app();
        app.on_event(key(KeyCode::Char('3')));
        app.on_event(key(KeyCode::Char('c')));
        // Jump straight to the last field and submit empty.
        app.create_form.cursor = 2;
        let actions = app.on_event(key(KeyCode::Enter));
        assert!(actions.is_empty());
        assert!(matches!(
            app.toast.as_ref().map(|t| t.severity),
            Some(Severity::Error)
        ));
        assert_eq!(app.focus, Focus::CreateForm);
    }

    #[test]
    fn join_form_validates_network_id() {
        let mut app = make_app();
        app.on_event(key(KeyCode::Char('3')));
        app.on_event(key(KeyCode::Char('j')));
        for c in "abc".chars() {
            app.on_event(key(KeyCode::Char(c)));
        }
        app.on_event(key(KeyCode::Tab));
        for c in "pw".chars() {
            app.on_event(key(KeyCode::Char(c)));
        }
        let actions = app.on_event(key(KeyCode::Enter));
        assert!(actions.is_empty());
        assert!(app.toast.as_ref().unwrap().message.contains("number"));

        // Fix the id and resubmit.
        app.join_form.set(0, "7");
        app.join_form.cursor = 1;
        let actions = app.on_event(key(KeyCode::Enter));
        assert_eq!(
            actions,
            vec![Action::JoinNetwork {
                network_id: 7,
                password: "pw".into(),
            }]
        );
    }

    #[test]
    fn esc_closes_form_without_submitting() {
        let mut app = make_app();
        app.on_event(key(KeyCode::Char('3')));
        app.on_event(key(KeyCode::Char('j')));
        app.on_event(key(KeyCode::Esc));
        assert_eq!(app.focus, Focus::Tabs);
        assert!(!app.busy);
    }

    #[test]
    fn directory_selection_and_prefill() {
        let mut app = make_app();
        let dir = vec![
            PublicNetworkInfo {
                network_id: NetworkId(11),
                name: "alpha".into(),
                subnet: "26.0.11.0/24".into(),
                member_count: 3,
                is_public: true,
            },
            PublicNetworkInfo {
                network_id: NetworkId(22),
                name: "beta".into(),
                subnet: "26.0.22.0/24".into(),
                member_count: 1,
                is_public: false,
            },
        ];
        app.on_event(Event::Directory(Ok(dir)));
        assert_eq!(app.dir_selected, 0);

        app.on_event(key(KeyCode::Char('3')));
        app.on_event(key(KeyCode::Down)); // enter list focus
        assert_eq!(app.focus, Focus::DirList);
        app.on_event(key(KeyCode::Down));
        assert_eq!(app.dir_selected, 1);
        app.on_event(key(KeyCode::Down)); // clamped at the end
        assert_eq!(app.dir_selected, 1);
        app.on_event(key(KeyCode::Up));
        assert_eq!(app.dir_selected, 0);

        app.on_event(key(KeyCode::Enter)); // prefill join form
        assert_eq!(app.focus, Focus::JoinForm);
        assert_eq!(app.join_form.value(0), "11");
    }

    #[test]
    fn snapshot_updates_rates_and_clears_error() {
        let mut app = make_app();
        app.on_event(Event::Snapshot(Err("offline".into())));
        assert!(app.daemon_offline());

        app.on_event(Event::Snapshot(Ok(sample_snapshot(1000, 2000))));
        assert!(!app.daemon_offline());
        assert!(app.tx_rate.is_empty()); // first sample has no delta

        // The two snapshots arrive back to back, so the rate divider hits the
        // 0.05 s anti-jitter clamp: 2000 B / 0.05 s = 40000 B/s.
        app.on_event(Event::Snapshot(Ok(sample_snapshot(3000, 2000))));
        assert_eq!(app.tx_rate.last(), Some(&40000));
        assert_eq!(app.rx_rate.last(), Some(&0));
        assert_eq!(app.snapshot.as_ref().unwrap().status.network_id.0, 1);
    }

    #[test]
    fn coord_result_clears_busy_and_toasts() {
        let mut app = make_app();
        app.busy = true;
        app.on_event(Event::Coord(
            Action::CreateNetwork {
                name: "x".into(),
                password: "y".into(),
                public: false,
            },
            Ok(crate::coord::CoordOutcome::Created(
                lanlink_proto::CreateNetworkResponse {
                    network_id: NetworkId(9),
                    subnet: "26.0.9.0/24".into(),
                    is_public: false,
                },
            )),
        ));
        assert!(!app.busy);
        assert!(app.toast.as_ref().unwrap().message.contains("id 9"));

        app.busy = true;
        app.on_event(Event::Coord(
            Action::JoinNetwork {
                network_id: 1,
                password: "z".into(),
            },
            Err("boom".into()),
        ));
        assert!(!app.busy);
        assert!(app.toast.as_ref().unwrap().message.contains("boom"));
    }

    #[test]
    fn non_press_key_events_ignored() {
        let mut app = make_app();
        let mut ev = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        ev.kind = KeyEventKind::Release;
        app.on_event(Event::Key(ev));
        assert!(!app.quit);
    }

    #[test]
    fn sorted_peers_orders_by_latency() {
        let mut app = make_app();
        let mut snap = sample_snapshot(0, 0);
        snap.peers.push(PeerStatus {
            node_id: NodeId([0, 0, 0, 0, 0, 0, 0, 3]),
            virtual_ip: VirtualIp("26.0.0.3".parse().unwrap()),
            endpoint: None,
            path: PathKind::Punching,
            latency_ms: None,
            last_seen_secs_ago: None,
            tx_bytes: 0,
            rx_bytes: 0,
            is_gateway: false,
            shared_subnet: String::new(),
        });
        app.on_event(Event::Snapshot(Ok(snap)));

        let peers = app.sorted_peers();
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0].latency_ms, Some(7)); // measured first
        assert_eq!(peers[1].latency_ms, None); // unmeasured last
    }
}

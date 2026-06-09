use eframe::egui::{self, Color32, RichText};
use egui_plot::{Line, Plot, PlotPoints};
use std::collections::{HashMap, VecDeque};
use sysinfo::{Pid, Process, ProcessesToUpdate, System, Users};

const MAX_HISTORY: usize = 60;
const CPU_REFRESH_MS: u64 = 2000;
const PROC_REFRESH_MS: u64 = 5000;
const ROW_HEIGHT: f32 = 20.0;

#[derive(PartialEq, Clone, Copy)]
enum SortCol {
    Pid,
    User,
    Name,
    Cpu,
    MemBytes,
    MemPct,
    Status,
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Overview,
    Processes,
}

#[derive(Clone)]
struct ProcRow {
    pid: Pid,
    user: String,
    name: String,
    name_lower: String,
    cpu: f32,
    mem_bytes: u64,
    mem_pct: f32,
    status: &'static str,
}

struct SysMonitor {
    sys: System,
    user_cache: HashMap<sysinfo::Uid, String>,
    tab: Tab,
    tab_changed: bool,
    sort_col: SortCol,
    sort_ascending: bool,
    filter: String,
    filter_lower: String,
    processes: Vec<ProcRow>,
    total_sys_processes: usize,
    cpu_history: VecDeque<f64>,
    mem_history: VecDeque<f64>,
    cpu_pts_cache: Vec<[f64; 2]>,
    mem_pts_cache: Vec<[f64; 2]>,
    pts_dirty: bool,
    last_cpu_refresh: std::time::Instant,
    last_proc_refresh: std::time::Instant,
    need_sort: bool,
}

impl SysMonitor {
    fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        let users = Users::new_with_refreshed_list();
        let mut app = Self {
            user_cache: build_user_cache(&users),
            processes: Vec::with_capacity(512),
            total_sys_processes: 0,
            sys,
            tab: Tab::Overview,
            tab_changed: false,
            sort_col: SortCol::Cpu,
            sort_ascending: false,
            filter: String::new(),
            filter_lower: String::new(),
            cpu_history: VecDeque::with_capacity(MAX_HISTORY),
            mem_history: VecDeque::with_capacity(MAX_HISTORY),
            cpu_pts_cache: Vec::with_capacity(MAX_HISTORY),
            mem_pts_cache: Vec::with_capacity(MAX_HISTORY),
            pts_dirty: true,
            last_cpu_refresh: std::time::Instant::now(),
            last_proc_refresh: std::time::Instant::now() - std::time::Duration::from_millis(PROC_REFRESH_MS * 2),
            need_sort: false,
        };
        app.refresh_cpu();
        app.refresh_processes();
        app
    }

    fn refresh_cpu(&mut self) {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        let cpu = self.sys.global_cpu_usage() as f64;
        self.cpu_history.push_back(cpu);
        if self.cpu_history.len() > MAX_HISTORY {
            self.cpu_history.pop_front();
        }
        let total = self.sys.total_memory() as f64;
        let used = self.sys.used_memory() as f64;
        let mem_pct = if total > 0.0 { (used / total) * 100.0 } else { 0.0 };
        self.mem_history.push_back(mem_pct);
        if self.mem_history.len() > MAX_HISTORY {
            self.mem_history.pop_front();
        }
        self.pts_dirty = true;
        self.last_cpu_refresh = std::time::Instant::now();
    }

    fn refresh_processes(&mut self) {
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        self.sys.refresh_memory();
        self.update_process_list();
        self.last_proc_refresh = std::time::Instant::now();
    }

    fn maybe_refresh_processes(&mut self) {
        let on_processes = self.tab == Tab::Processes;
        let stale = self.last_proc_refresh.elapsed().as_millis() >= PROC_REFRESH_MS as u128;
        let needs = (stale && on_processes) || (self.tab_changed && on_processes);
        if needs {
            self.refresh_processes();
            self.tab_changed = false;
        }
    }

    fn rebuild_plot_points(&mut self) {
        if !self.pts_dirty {
            return;
        }
        self.cpu_pts_cache.clear();
        self.cpu_pts_cache.extend(
            self.cpu_history.iter().enumerate().map(|(i, &v)| [i as f64, v]),
        );
        self.mem_pts_cache.clear();
        self.mem_pts_cache.extend(
            self.mem_history.iter().enumerate().map(|(i, &v)| [i as f64, v]),
        );
        self.pts_dirty = false;
    }

    fn update_process_list(&mut self) {
        let total_mem = self.sys.total_memory() as f32;
        self.processes.clear();

        for (pid, proc_) in self.sys.processes() {
            let name = proc_.name().to_string_lossy().to_string();
            let name_lower = name.to_lowercase();

            if !self.filter_lower.is_empty() && !name_lower.contains(&self.filter_lower) {
                continue;
            }

            let user = self.get_user_name_cached(proc_);
            let cpu = proc_.cpu_usage();
            let mem_bytes = proc_.memory();
            let mem_pct = if total_mem > 0.0 { (mem_bytes as f32 / total_mem) * 100.0 } else { 0.0 };
            let status = status_str(proc_.status());

            self.processes.push(ProcRow { pid: *pid, user, name, name_lower, cpu, mem_bytes, mem_pct, status });
        }

        self.total_sys_processes = self.processes.len();
        self.sort_processes();
    }

    fn sort_processes(&mut self) {
        let asc = self.sort_ascending;
        let col = self.sort_col;
        self.processes.sort_by(|a, b| {
            let ord = match col {
                SortCol::Pid => a.pid.cmp(&b.pid),
                SortCol::User => a.user.cmp(&b.user),
                SortCol::Name => a.name_lower.cmp(&b.name_lower),
                SortCol::Cpu => a.cpu.partial_cmp(&b.cpu).unwrap_or(std::cmp::Ordering::Equal),
                SortCol::MemBytes => a.mem_bytes.cmp(&b.mem_bytes),
                SortCol::MemPct => a.mem_pct.partial_cmp(&b.mem_pct).unwrap_or(std::cmp::Ordering::Equal),
                SortCol::Status => a.status.cmp(b.status),
            };
            if asc { ord } else { ord.reverse() }
        });
    }

    fn get_user_name_cached(&self, proc_: &Process) -> String {
        if let Some(uid) = proc_.user_id() {
            if let Some(name) = self.user_cache.get(uid) {
                return name.clone();
            }
        }
        "-".to_string()
    }
}

fn build_user_cache(users: &Users) -> HashMap<sysinfo::Uid, String> {
    let mut map = HashMap::new();
    for user in users.list() {
        map.insert(user.id().clone(), user.name().to_string());
    }
    map
}

fn status_str(s: sysinfo::ProcessStatus) -> &'static str {
    match s {
        sysinfo::ProcessStatus::Run => "Run",
        sysinfo::ProcessStatus::Sleep => "Sleep",
        sysinfo::ProcessStatus::Idle => "Idle",
        sysinfo::ProcessStatus::Zombie => "Zombie",
        sysinfo::ProcessStatus::Stop => "Stop",
        sysinfo::ProcessStatus::Dead => "Dead",
        sysinfo::ProcessStatus::Parked => "Parked",
        _ => "?",
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB { format!("{:.1}G", bytes as f64 / GB as f64) }
    else if bytes >= MB { format!("{:.1}M", bytes as f64 / MB as f64) }
    else if bytes >= KB { format!("{:.0}K", bytes as f64 / KB as f64) }
    else { format!("{}B", bytes) }
}

impl eframe::App for SysMonitor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = std::time::Instant::now();
        if now.duration_since(self.last_cpu_refresh).as_millis() >= CPU_REFRESH_MS as u128 {
            self.refresh_cpu();
        }
        self.maybe_refresh_processes();
        self.rebuild_plot_points();
        ctx.request_repaint_after(std::time::Duration::from_millis(CPU_REFRESH_MS));

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("SysMonitor").color(Color32::from_rgb(0, 200, 255)).strong());
                ui.separator();
                let uptime = System::uptime();
                let d = uptime / 86400;
                let h = (uptime % 86400) / 3600;
                let m = (uptime % 3600) / 60;
                ui.label(format!("Up: {}d{}h{}m", d, h, m));
                ui.separator();
                let load = System::load_average();
                ui.label(format!("Load: {:.1} {:.1} {:.1}", load.one, load.five, load.fifteen));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{} procs", self.total_sys_processes)).color(Color32::YELLOW));
                });
            });
        });

        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            let old = self.tab;
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Overview, "Visao Geral");
                ui.selectable_value(&mut self.tab, Tab::Processes, "Processos");
            });
            self.tab_changed = old != self.tab;
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.tab {
                Tab::Overview => self.draw_overview(ui),
                Tab::Processes => self.draw_processes(ui),
            }
        });
    }
}

impl SysMonitor {
    fn draw_overview(&mut self, ui: &mut egui::Ui) {
        let cpu_global = self.sys.global_cpu_usage();
        let total_mem = self.sys.total_memory();
        let used_mem = self.sys.used_memory();
        let avail_mem = self.sys.available_memory();
        let free_mem = self.sys.free_memory();
        let total_swap = self.sys.total_swap();
        let used_swap = self.sys.used_swap();

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading(RichText::new("CPU").color(Color32::from_rgb(100, 220, 100)));
            ui.add(egui::ProgressBar::new(cpu_global / 100.0)
                .text(format!("Total: {:.0}%", cpu_global)).fill(cpu_color(cpu_global)));
            ui.add_space(4.0);

            let cpus = self.sys.cpus();
            let num = cpus.len();
            let cols = if num <= 8 { 2 } else if num <= 16 { 4 } else { 8 };
            egui::Grid::new("cpu_cores").num_columns(cols).spacing([8.0, 2.0]).show(ui, |ui| {
                for (i, cpu) in cpus.iter().enumerate() {
                    let usage = cpu.cpu_usage();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("C{}", i)).monospace().size(10.0));
                        ui.add(egui::ProgressBar::new(usage / 100.0).desired_width(70.0)
                            .fill(cpu_color(usage)).text(format!("{:.0}%", usage)));
                    });
                    if (i + 1) % cols == 0 { ui.end_row(); }
                }
            });

            ui.add_space(8.0);
            ui.label(RichText::new("Historico CPU (%)").strong());
            Plot::new("cpu_plot").height(100.0).view_aspect(4.0).include_y(0.0).include_y(100.0).show_x(false)
                .show(ui, |plot_ui| {
                    plot_ui.line(Line::new(PlotPoints::from(self.cpu_pts_cache.clone()))
                        .color(Color32::from_rgb(80, 220, 80)).width(2.0));
                });

            ui.separator();
            let mem_pct = if total_mem > 0 { (used_mem as f64 / total_mem as f64) * 100.0 } else { 0.0 };
            ui.heading(RichText::new("Memoria RAM").color(Color32::from_rgb(80, 180, 255)));
            ui.add(egui::ProgressBar::new((mem_pct / 100.0) as f32)
                .text(format!("{} / {} ({:.0}%)", format_bytes(used_mem), format_bytes(total_mem), mem_pct))
                .fill(mem_color(mem_pct)));
            ui.label(format!("Disp: {}  Livre: {}", format_bytes(avail_mem), format_bytes(free_mem)));

            if total_swap > 0 {
                ui.separator();
                ui.heading(RichText::new("Swap").color(Color32::from_rgb(200, 100, 220)));
                let spct = (used_swap as f64 / total_swap as f64) * 100.0;
                ui.add(egui::ProgressBar::new((spct / 100.0) as f32)
                    .text(format!("{} / {}", format_bytes(used_swap), format_bytes(total_swap)))
                    .fill(Color32::from_rgb(200, 100, 220)));
            }

            ui.separator();
            ui.add_space(4.0);
            let top = self.top_n(10, true);
            ui.heading(RichText::new(format!("Top {} CPU", top.len())).color(Color32::YELLOW));
            self.draw_small_list(ui, &top, true);
            ui.add_space(4.0);
            let top = self.top_n(10, false);
            ui.heading(RichText::new(format!("Top {} Memoria", top.len())).color(Color32::from_rgb(80, 180, 255)));
            self.draw_small_list(ui, &top, false);
        });
    }

    fn top_n(&self, n: usize, by_cpu: bool) -> Vec<&ProcRow> {
        let mut v: Vec<&ProcRow> = self.processes.iter().collect();
        if by_cpu {
            v.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal));
        } else {
            v.sort_by(|a, b| b.mem_bytes.cmp(&a.mem_bytes));
        }
        v.truncate(n);
        v
    }

    fn draw_small_list(&self, ui: &mut egui::Ui, rows: &[&ProcRow], by_cpu: bool) {
        if rows.is_empty() { ui.label("Sem dados"); return; }
        egui::Grid::new(if by_cpu { "t_cpu" } else { "t_mem" }).striped(true).num_columns(4).show(ui, |ui| {
            ui.label(RichText::new("PID").strong());
            ui.label(RichText::new("Nome").strong());
            if by_cpu { ui.label(RichText::new("CPU%").strong()); ui.label(RichText::new("MEM").strong()); }
            else { ui.label(RichText::new("MEM").strong()); ui.label(RichText::new("MEM%").strong()); }
            ui.end_row();
            for p in rows {
                ui.label(p.pid.to_string());
                ui.label(&p.name);
                if by_cpu {
                    let c = if p.cpu > 80.0 { Color32::RED } else if p.cpu > 40.0 { Color32::YELLOW } else { Color32::WHITE };
                    ui.label(RichText::new(format!("{:.1}", p.cpu)).color(c));
                    ui.label(format_bytes(p.mem_bytes));
                } else {
                    ui.label(format_bytes(p.mem_bytes));
                    ui.label(format!("{:.1}%", p.mem_pct));
                }
                ui.end_row();
            }
        });
    }

    fn draw_processes(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Filtro:");
            let resp = ui.text_edit_singleline(&mut self.filter);
            if resp.changed() {
                self.filter_lower = self.filter.to_lowercase();
                self.refresh_processes();
            }
            if ui.button("Limpar").clicked() {
                self.filter.clear(); self.filter_lower.clear(); self.refresh_processes();
            }
            ui.separator();
            ui.label(RichText::new(self.processes.len().to_string()).color(Color32::YELLOW));
        });
        ui.separator();

        let mut pids_to_kill: Vec<Pid> = Vec::new();
        let cw: [f32; 8] = [50.0, 70.0, 160.0, 55.0, 65.0, 55.0, 60.0, 28.0];

        ui.horizontal(|ui| {
            ui.set_width(ui.available_width());
            let labels = ["PID", "User", "Nome", "CPU%", "MEM", "MEM%", "Status", "X"];
            let cols = [SortCol::Pid, SortCol::User, SortCol::Name, SortCol::Cpu,
                        SortCol::MemBytes, SortCol::MemPct, SortCol::Status, SortCol::Pid];
            for i in 0..7 {
                let active = self.sort_col == cols[i];
                let arrow = if active { if self.sort_ascending { " ^" } else { " v" } } else { "" };
                let text = format!("{}{}", labels[i], arrow);
                if ui.add_sized([cw[i], ROW_HEIGHT], |ui: &mut egui::Ui| ui.selectable_label(active, &text)).clicked() {
                    if active { self.sort_ascending = !self.sort_ascending; }
                    else { self.sort_col = cols[i]; self.sort_ascending = false; }
                    self.need_sort = true;
                }
            }
            ui.add_sized([cw[7], ROW_HEIGHT], |ui: &mut egui::Ui| ui.label(RichText::new("X").strong()));
        });

        let n = self.processes.len();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, ROW_HEIGHT, n, |ui, range| {
                for i in range {
                    let p = &self.processes[i];
                    ui.horizontal(|ui| {
                        ui.set_width(ui.available_width());
                        ui.add_sized([cw[0], ROW_HEIGHT], |u: &mut egui::Ui| u.label(p.pid.to_string()));
                        ui.add_sized([cw[1], ROW_HEIGHT], |u: &mut egui::Ui| u.label(&p.user));
                        ui.add_sized([cw[2], ROW_HEIGHT], |u: &mut egui::Ui| u.label(&p.name));
                        let cc = if p.cpu > 80.0 { Color32::RED } else if p.cpu > 40.0 { Color32::YELLOW } else { Color32::WHITE };
                        ui.add_sized([cw[3], ROW_HEIGHT], |u: &mut egui::Ui| u.label(RichText::new(format!("{:.1}", p.cpu)).color(cc)));
                        ui.add_sized([cw[4], ROW_HEIGHT], |u: &mut egui::Ui| u.label(format_bytes(p.mem_bytes)));
                        let mc = if p.mem_pct > 50.0 { Color32::RED } else if p.mem_pct > 20.0 { Color32::YELLOW } else { Color32::WHITE };
                        ui.add_sized([cw[5], ROW_HEIGHT], |u: &mut egui::Ui| u.label(RichText::new(format!("{:.1}%", p.mem_pct)).color(mc)));
                        ui.add_sized([cw[6], ROW_HEIGHT], |u: &mut egui::Ui| u.label(p.status));
                        if ui.add_sized([cw[7], ROW_HEIGHT], |u: &mut egui::Ui| {
                            u.add(egui::Button::new("X").min_size(egui::vec2(cw[7], ROW_HEIGHT)))
                        }).clicked() {
                            pids_to_kill.push(p.pid);
                        }
                    });
                }
            });

        for pid in pids_to_kill {
            if let Some(proc_) = self.sys.process(pid) { proc_.kill(); }
        }

        if self.need_sort { self.sort_processes(); self.need_sort = false; }
    }
}

fn cpu_color(v: f32) -> Color32 {
    if v > 90.0 { Color32::RED } else if v > 60.0 { Color32::YELLOW } else { Color32::from_rgb(80, 200, 80) }
}

fn mem_color(v: f64) -> Color32 {
    if v > 90.0 { Color32::RED } else if v > 70.0 { Color32::YELLOW } else { Color32::from_rgb(80, 180, 255) }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SysMonitor")
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size([700.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native("SysMonitor", options, Box::new(|cc| {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        Ok(Box::new(SysMonitor::new()))
    }))
}

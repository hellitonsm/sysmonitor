use eframe::egui::{self, Color32, RichText, Rounding, Stroke, Ui, Vec2};
use std::collections::HashMap;
use std::fs;
use std::time::{Duration, Instant};

// ===== Tema moderno =====
mod theme {
    use super::*;
    pub const BG: Color32 = Color32::from_rgb(11, 12, 16);       // quase preto azulado
    pub const SURFACE: Color32 = Color32::from_rgb(18, 20, 28);
    pub const SURFACE2: Color32 = Color32::from_rgb(24, 26, 36);
    pub const ROW_EVEN: Color32 = Color32::from_rgb(17, 19, 26);
    pub const ROW_ODD: Color32 = Color32::from_rgb(22, 24, 32);
    pub const ACCENT: Color32 = Color32::from_rgb(99, 102, 241); // indigo-500
    pub const ACCENT_HOVER: Color32 = Color32::from_rgb(129, 140, 248);
    pub const TEXT: Color32 = Color32::from_rgb(226, 232, 240);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(148, 163, 184);
    pub const BORDER: Color32 = Color32::from_rgb(38, 42, 56);
}

use theme::*;

// ===== Modelo =====
#[derive(Debug, Clone)]
struct ProcInfo {
    pid: u32,
    name: String,
    state: char,
    vmsize_kb: u64,
    rss_kb: u64,
    utime: u64,
    stime: u64,
}

impl ProcInfo {
    fn total_time(&self) -> u64 {
        self.utime.saturating_add(self.stime)
    }
}

// ===== Leitura /proc =====
fn read_mem_total_kb() -> u64 {
    fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|c| {
            c.lines().find_map(|line| {
                line.strip_prefix("MemTotal:")
                    .and_then(|rest| rest.trim().strip_suffix(" kB"))
                    .and_then(|kb| kb.trim().parse::<u64>().ok())
            })
        })
        .unwrap_or(0)
}

fn read_num_cpus() -> usize {
    fs::read_to_string("/sys/devices/system/cpu/online")
        .ok()
        .and_then(|s| {
            s.trim().split_once('-').and_then(|(a, b)| {
                Some((b.parse::<u32>().ok()? - a.parse::<u32>().ok()? + 1) as usize)
            })
        })
        .or_else(|| {
            (0..).take_while(|i| fs::metadata(format!("/sys/devices/system/cpu/cpu{i}")).is_ok()).last().map(|n| n + 1)
        })
        .unwrap_or(1)
}

fn read_total_jiffies() -> u64 {
    fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|c| c.lines().next().map(|l| l.split_whitespace().skip(1).filter_map(|v| v.parse::<u64>().ok()).sum()))
        .unwrap_or(0)
}

fn read_proc(pid: u32) -> Option<ProcInfo> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let mut name = String::new();
    let mut state = '?';
    let mut vmsize = 0;
    let mut rss = 0;

    for line in status.lines() {
        if let Some(v) = line.strip_prefix("Name:") {
            name = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("State:") {
            state = v.trim().chars().next().unwrap_or('?');
        } else if let Some(v) = line.strip_prefix("VmSize:") {
            vmsize = v.trim().split_whitespace().next()?.parse().ok()?;
        } else if let Some(v) = line.strip_prefix("VmRSS:") {
            rss = v.trim().split_whitespace().next()?.parse().ok()?;
        }
    }
    if name.is_empty() { return None; }

    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let idx = stat.rfind(')')?;
    let fields: Vec<&str> = stat[idx + 2..].split_whitespace().collect();
    let utime = fields.get(11)?.parse().ok()?;
    let stime = fields.get(12)?.parse().ok()?;

    Some(ProcInfo { pid, name, state, vmsize_kb: vmsize, rss_kb: rss, utime, stime })
}

fn gather_pids() -> Vec<u32> {
    fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok()?.parse().ok())
        .collect()
}

// ===== Utilidades =====
#[derive(Clone, Copy, PartialEq, Eq)]
enum SortCol { Pid, Name, Ram, Cpu, VmSize, State }

fn fmt_ram(kb: u64) -> String {
    const GB: f64 = 1_048_576.0;
    const MB: f64 = 1024.0;
    match kb {
        k if k as f64 >= GB => format!("{:.1} GB", k as f64 / GB),
        k if k as f64 >= MB => format!("{:.1} MB", k as f64 / MB),
        k => format!("{k} KB"),
    }
}

fn state_info(c: char) -> (&'static str, Color32) {
    match c {
        'R' => ("Running", Color32::from_rgb(74, 222, 128)),
        'S' => ("Sleep", Color32::from_rgb(96, 165, 250)),
        'D' => ("Disk", Color32::from_rgb(250, 204, 21)),
        'Z' => ("Zombie", Color32::from_rgb(248, 113, 113)),
        'T' => ("Stopped", Color32::from_rgb(167, 139, 250)),
        'I' => ("Idle", Color32::from_rgb(100, 116, 139)),
        _ => ("Other", TEXT_DIM),
    }
}

fn cpu_color(pct: f64, ncpu: usize) -> Color32 {
    let r = (pct / (ncpu as f64 * 100.0).max(1.0)).clamp(0.0, 1.0);
    match r {
        r if r < 0.15 => Color32::from_rgb(74, 222, 128),
        r if r < 0.40 => Color32::from_rgb(134, 239, 172),
        r if r < 0.65 => Color32::from_rgb(250, 204, 21),
        r if r < 0.85 => Color32::from_rgb(251, 146, 60),
        _ => Color32::from_rgb(248, 113, 113),
    }
}

// ===== App =====
struct ProcMonitorApp {
    procs: Vec<ProcInfo>,
    prev_cpu: HashMap<u32, u64>,
    prev_jiffies: u64,
    cpu_vals: HashMap<u32, f64>,
    sort_col: SortCol,
    sort_asc: bool,
    search: String,
    last_update: Instant,
    interval: Duration,
    num_cpus: usize,
    total_ram_kb: u64,
}

impl ProcMonitorApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            procs: Vec::new(),
            prev_cpu: HashMap::new(),
            prev_jiffies: read_total_jiffies(),
            cpu_vals: HashMap::new(),
            sort_col: SortCol::Cpu,
            sort_asc: false,
            search: String::new(),
            last_update: Instant::now() - Duration::from_secs(2),
            interval: Duration::from_secs(1),
            num_cpus: read_num_cpus(),
            total_ram_kb: read_mem_total_kb(),
        }
    }

    fn refresh(&mut self) {
        let cur_jiffies = read_total_jiffies();
        let delta = (cur_jiffies.saturating_sub(self.prev_jiffies)) as f64;
        let delta = delta.max(1.0);

        let mut procs = Vec::new();
        let mut new_prev = HashMap::new();
        let mut cpu_vals = HashMap::new();

        for pid in gather_pids() {
            if let Some(p) = read_proc(pid) {
                let cur = p.total_time();
                let prev = self.prev_cpu.get(&pid).copied().unwrap_or(cur);
                let pct = (cur.saturating_sub(prev) as f64 / delta) * 100.0 * self.num_cpus as f64;
                cpu_vals.insert(pid, pct);
                new_prev.insert(pid, cur);
                procs.push(p);
            }
        }

        self.procs = procs;
        self.prev_cpu = new_prev;
        self.prev_jiffies = cur_jiffies;
        self.cpu_vals = cpu_vals;
        self.last_update = Instant::now();
    }

    fn filtered_sorted(&self) -> Vec<&ProcInfo> {
        let q = self.search.trim().to_lowercase();
        let mut v: Vec<&ProcInfo> = if q.is_empty() {
            self.procs.iter().collect()
        } else {
            self.procs.iter().filter(|p| p.name.to_lowercase().contains(&q) || p.pid.to_string().contains(&q)).collect()
        };

        v.sort_unstable_by(|a, b| {
            let ord = match self.sort_col {
                SortCol::Pid => a.pid.cmp(&b.pid),
                SortCol::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortCol::Ram => a.rss_kb.cmp(&b.rss_kb),
                SortCol::Cpu => self.cpu_vals.get(&a.pid).unwrap_or(&0.0).partial_cmp(self.cpu_vals.get(&b.pid).unwrap_or(&0.0)).unwrap_or(std::cmp::Ordering::Equal),
                SortCol::VmSize => a.vmsize_kb.cmp(&b.vmsize_kb),
                SortCol::State => a.state.cmp(&b.state),
            };
            if self.sort_asc { ord } else { ord.reverse() }
        });
        v
    }
}

impl eframe::App for ProcMonitorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.last_update.elapsed() >= self.interval {
            self.refresh();
        }
        ctx.request_repaint_after(Duration::from_millis(200));

        egui::CentralPanel::default().frame(egui::Frame::none().fill(BG).inner_margin(12.0)).show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(8.0);
            header(ui, self);
            toolbar(ui, self);
            ram_bar(ui, self);
            table(ui, self);
            footer(ui, self);
        });
    }
}

// ===== UI Components =====
fn header(ui: &mut Ui, app: &ProcMonitorApp) {
    egui::Frame::none().fill(SURFACE).rounding(Rounding::same(12.0)).stroke(Stroke::new(1.0, BORDER))
        .inner_margin(12.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("⬢").size(22.0).color(ACCENT).strong());
                ui.heading(RichText::new("SysMonitor").size(20.0).color(TEXT));
                ui.label(RichText::new("Linux").size(12.0).color(TEXT_DIM));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let used = app.procs.iter().map(|p| p.rss_kb).sum::<u64>();
                    let pct = used as f64 / app.total_ram_kb.max(1) as f64 * 100.0;
                    ui.label(RichText::new(format!("{} cores", app.num_cpus)).color(TEXT_DIM).size(12.0));
                    ui.separator();
                    ui.label(RichText::new(format!("RAM {} / {} ({:.0}%)", fmt_ram(used), fmt_ram(app.total_ram_kb), pct)).color(ACCENT_HOVER).size(12.0));
                });
            });
        });
}

fn toolbar(ui: &mut Ui, app: &mut ProcMonitorApp) {
    egui::Frame::none().fill(SURFACE).rounding(Rounding::same(12.0)).stroke(Stroke::new(1.0, BORDER))
        .inner_margin(8.0).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("🔍").size(14.0));
                ui.add(egui::TextEdit::singleline(&mut app.search).hint_text("Buscar por nome ou PID...").desired_width(200.0).text_color(TEXT));

                ui.add_space(12.0);
                ui.label(RichText::new("Atualizar").color(TEXT_DIM).size(11.0));
                for (lbl, ms) in [("0.5s", 500), ("1s", 1000), ("2s", 2000), ("5s", 5000)] {
                    let sel = app.interval == Duration::from_millis(ms);
                    if pill(ui, lbl, sel).clicked() { app.interval = Duration::from_millis(ms); }
                }

                ui.add_space(12.0);
                ui.label(RichText::new("Ordenar").color(TEXT_DIM).size(11.0));
                for (lbl, col) in [( "PID", SortCol::Pid), ("Nome", SortCol::Name), ("RAM", SortCol::Ram), ("CPU", SortCol::Cpu), ("VM", SortCol::VmSize), ("Estado", SortCol::State)] {
                    let active = app.sort_col == col;
                    let arrow = if active { if app.sort_asc { " ▲" } else { " ▼" } } else { "" };
                    if pill(ui, &format!("{lbl}{arrow}"), active).clicked() {
                        if active { app.sort_asc = !app.sort_asc; } else { app.sort_col = col; app.sort_asc = matches!(col, SortCol::Pid | SortCol::Name); }
                    }
                }
            });
        });
}

fn pill(ui: &mut Ui, label: &str, active: bool) -> egui::Response {
    let bg = if active { ACCENT } else { SURFACE2 };
    let fg = if active { Color32::WHITE } else { TEXT_DIM };
    ui.add(egui::Button::new(RichText::new(label).size(12.0).color(fg)).fill(bg).stroke(Stroke::new(1.0, BORDER)).rounding(Rounding::same(8.0)).min_size(Vec2::new(0.0, 24.0)))
}

fn ram_bar(ui: &mut Ui, app: &ProcMonitorApp) {
    let used = app.procs.iter().map(|p| p.rss_kb).sum::<u64>();
    let pct = (used as f32 / app.total_ram_kb.max(1) as f32).clamp(0.0, 1.0);
    egui::Frame::none().fill(SURFACE).rounding(Rounding::same(10.0)).stroke(Stroke::new(1.0, BORDER)).inner_margin(8.0).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("RAM").strong().size(11.0).color(TEXT_DIM));
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width() - 10.0, 18.0), egui::Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 6.0, SURFACE2);
            painter.rect_filled(egui::Rect::from_min_size(rect.min, Vec2::new(rect.width()*pct, rect.height())), 6.0, ACCENT);
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, format!("{:.0}% • {} / {}", pct*100.0, fmt_ram(used), fmt_ram(app.total_ram_kb)), egui::FontId::proportional(11.0), TEXT);
        });
    });
}

fn table(ui: &mut Ui, app: &mut ProcMonitorApp) {
    let items = app.filtered_sorted();
    egui::Frame::none().fill(SURFACE).rounding(Rounding::same(12.0)).stroke(Stroke::new(1.0, BORDER)).inner_margin(6.0).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink([false,false]).show_rows(ui, 26.0, items.len(), |ui, range| {
            // header
            if range.start == 0 {
                ui.horizontal(|ui| {
                    for (w, t) in [(28.0,""), (70.0,"PID"), (220.0,"NOME"), (100.0,"ESTADO"), (100.0,"RAM"), (100.0,"VM"), (80.0,"CPU%")] {
                        ui.add_sized([w,20.0], egui::Label::new(RichText::new(t).size(11.0).color(TEXT_DIM).strong()));
                    }
                });
                ui.separator();
            }
            for i in range {
                let p = items[i];
                let bg = if i % 2 == 0 { ROW_EVEN } else { ROW_ODD };
                egui::Frame::none().fill(bg).rounding(Rounding::same(6.0)).inner_margin(2.0).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.add_sized([28.0,22.0], egui::Button::new("✕").fill(Color32::TRANSPARENT).stroke(Stroke::NONE)).on_hover_text("Encerrar processo").clicked() {
                            let _ = std::process::Command::new("kill").arg(p.pid.to_string()).spawn();
                        }
                        ui.add_sized([70.0,22.0], egui::Label::new(RichText::new(p.pid.to_string()).monospace().color(TEXT_DIM)));
                        ui.add_sized([220.0,22.0], egui::Label::new(RichText::new(&p.name).color(TEXT).size(12.0))).on_hover_text(&p.name);
                        let (st_txt, st_col) = state_info(p.state);
                        ui.add_sized([100.0,22.0], egui::Label::new(RichText::new(format!("● {st_txt}")).color(st_col).size(11.0)));
                        ui.add_sized([100.0,22.0], egui::Label::new(RichText::new(fmt_ram(p.rss_kb)).color(TEXT)));
                        ui.add_sized([100.0,22.0], egui::Label::new(RichText::new(fmt_ram(p.vmsize_kb)).color(TEXT_DIM)));
                        let cpu = app.cpu_vals.get(&p.pid).copied().unwrap_or(0.0);
                        ui.add_sized([80.0,22.0], egui::Label::new(RichText::new(format!("{cpu:.1}%")).color(cpu_color(cpu, app.num_cpus)).strong()));
                    });
                });
            }
        });
    });
}

fn footer(ui: &mut Ui, app: &ProcMonitorApp) {
    egui::Frame::none().fill(SURFACE).rounding(Rounding::same(10.0)).stroke(Stroke::new(1.0, BORDER)).inner_margin(8.0).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} processos", app.procs.len())).size(11.0).color(TEXT_DIM));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(format!("atualizado há {}ms", app.last_update.elapsed().as_millis())).size(11.0).color(TEXT_DIM));
            });
        });
    });
}

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([900.0, 600.0]).with_min_inner_size([720.0, 420.0]).with_title("SysMonitor — Rust"),
        ..Default::default()
    };
    eframe::run_native("SysMonitor", options, Box::new(|cc| {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG;
        visuals.window_fill = SURFACE;
        visuals.widgets.noninteractive.bg_fill = SURFACE;
        visuals.widgets.inactive.bg_fill = SURFACE2;
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(35,38,52);
        visuals.widgets.active.bg_fill = ACCENT;
        visuals.selection.bg_fill = ACCENT;
        visuals.window_rounding = Rounding::same(12.0);
        visuals.window_stroke = Stroke::new(1.0, BORDER);
        cc.egui_ctx.set_visuals(visuals);
        cc.egui_ctx.set_pixels_per_point(1.1);
        Ok(Box::new(ProcMonitorApp::new(cc)))
    }))
}

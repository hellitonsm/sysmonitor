#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::{
    egui::{self, Color32, RichText, Rounding, Stroke, Ui, Vec2},
    App,
};
use std::collections::HashMap;
use std::fs;
use std::time::{Duration, Instant};

const BG: Color32 = Color32::from_rgb(15, 15, 25);
const SURFACE: Color32 = Color32::from_rgb(22, 22, 35);
const ROW_EVEN: Color32 = Color32::from_rgb(20, 20, 33);
const ROW_ODD: Color32 = Color32::from_rgb(26, 26, 42);
const ACCENT: Color32 = Color32::from_rgb(80, 140, 255);
const ACCENT_DIM: Color32 = Color32::from_rgb(50, 100, 200);
const TEXT: Color32 = Color32::from_rgb(220, 220, 230);
const TEXT_DIM: Color32 = Color32::from_rgb(140, 140, 165);


struct ProcInfo {
    pid: u32,
    name: String,
    state: char,
    vmsize_kb: u64,
    rss_kb: u64,
    utime: u64,
    stime: u64,
}

fn read_total_ram_kb() -> u64 {
    if let Ok(c) = fs::read_to_string("/proc/meminfo") {
        for line in c.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                if let Some(kb) = rest.trim().strip_suffix(" kB") {
                    return kb.trim().parse().unwrap_or(0);
                }
            }
        }
    }
    0
}

fn read_num_cpus() -> usize {
    let online = fs::read_to_string("/sys/devices/system/cpu/online").ok();
    if let Some(s) = &online {
        if let Some((a, b)) = s.trim().split_once('-') {
            if let (Ok(x), Ok(y)) = (a.parse::<u32>(), b.parse::<u32>()) {
                return ((y - x + 1) as usize).max(1);
            }
        }
    }
    let mut n = 0usize;
    while fs::metadata(format!("/sys/devices/system/cpu/cpu{}", n)).is_ok() {
        n += 1;
    }
    n.max(1)
}

fn read_total_jiffies() -> Option<u64> {
    let c = fs::read_to_string("/proc/stat").ok()?;
    c.lines()
        .next()
        .map(|l| {
            l.split_whitespace()
                .skip(1)
                .filter_map(|v| v.parse::<u64>().ok())
                .sum()
        })
}

fn read_proc(pid: u32) -> Option<ProcInfo> {
    let status = fs::read_to_string(format!("/proc/{}/status", pid)).ok()?;
    let mut name = String::new();
    let mut state: char = '?';
    let mut vmsize: u64 = 0;
    let mut rss: u64 = 0;

    for line in status.lines() {
        if let Some(v) = line.strip_prefix("Name:\t") {
            name = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("State:\t") {
            state = v.trim().chars().next().unwrap_or('?');
        } else if let Some(v) = line.strip_prefix("VmSize:\t") {
            vmsize = v.trim().split_whitespace().next()?.parse().ok()?;
        } else if let Some(v) = line.strip_prefix("VmRSS:\t") {
            rss = v.trim().split_whitespace().next()?.parse().ok()?;
        }
    }

    if name.is_empty() {
        return None;
    }

    let stat = fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
    let rest = stat.rfind(')')?;
    let fields: Vec<&str> = stat[rest + 2..].split_whitespace().collect();
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;

    Some(ProcInfo {
        pid,
        name,
        state,
        vmsize_kb: vmsize,
        rss_kb: rss,
        utime,
        stime,
    })
}

fn gather_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    if let Ok(entries) = fs::read_dir("/proc") {
        for e in entries.flatten() {
            if let Ok(n) = e.file_name().into_string() {
                if let Ok(pid) = n.parse::<u32>() {
                    pids.push(pid);
                }
            }
        }
    }
    pids.sort();
    pids
}

#[derive(Clone, Copy, PartialEq)]
enum SortCol {
    Pid,
    Name,
    Ram,
    Cpu,
    VmSize,
    State,
}

struct ProcMonitorApp {
    procs: Vec<ProcInfo>,
    prev_cpu: HashMap<u32, (u64, u64)>,
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
        let num_cpus = read_num_cpus();
        let total_ram_kb = read_total_ram_kb();
        let prev_jiffies = read_total_jiffies().unwrap_or(0);

        Self {
            procs: Vec::new(),
            prev_cpu: HashMap::new(),
            prev_jiffies,
            cpu_vals: HashMap::new(),
            sort_col: SortCol::Ram,
            sort_asc: false,
            search: String::new(),
            last_update: Instant::now(),
            interval: Duration::from_secs(1),
            num_cpus,
            total_ram_kb,
        }
    }

    fn refresh(&mut self) {
        let cur_jiffies = read_total_jiffies().unwrap_or(self.prev_jiffies);
        let pids = gather_pids();
        let mut procs = Vec::with_capacity(pids.len());
        let mut new_cpu: HashMap<u32, (u64, u64)> = HashMap::new();
        let mut cpu_vals: HashMap<u32, f64> = HashMap::new();
        let delta = (cur_jiffies as f64 - self.prev_jiffies as f64).max(1.0);

        for pid in &pids {
            if let Some(p) = read_proc(*pid) {
                let cpu = cpu_pct(&p, &self.prev_cpu, delta);
                cpu_vals.insert(*pid, cpu);
                new_cpu.insert(*pid, (p.utime, p.stime));
                procs.push(p);
            }
        }

        self.procs = procs;
        self.prev_cpu = new_cpu;
        self.prev_jiffies = cur_jiffies;
        self.cpu_vals = cpu_vals;
        self.last_update = Instant::now();
    }

    fn sorted(&self) -> Vec<&ProcInfo> {
        let q = self.search.to_lowercase();

        let mut v: Vec<&ProcInfo> = if q.is_empty() {
            self.procs.iter().collect()
        } else {
            self.procs
                .iter()
                .filter(|p| {
                    p.name.to_lowercase().contains(&q) || p.pid.to_string().contains(&q)
                })
                .collect()
        };

        v.sort_by(|a, b| {
            let ord = match self.sort_col {
                SortCol::Pid => a.pid.cmp(&b.pid),
                SortCol::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortCol::Ram => a.rss_kb.cmp(&b.rss_kb),
                SortCol::Cpu => {
                    let ca = self.cpu_vals.get(&a.pid).copied().unwrap_or(0.0);
                    let cb = self.cpu_vals.get(&b.pid).copied().unwrap_or(0.0);
                    ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
                }
                SortCol::VmSize => a.vmsize_kb.cmp(&b.vmsize_kb),
                SortCol::State => a.state.cmp(&b.state),
            };
            if self.sort_asc { ord } else { ord.reverse() }
        });

        v
    }

    fn ram_used_kb(&self) -> u64 {
        self.procs.iter().map(|p| p.rss_kb).sum()
    }
}

fn cpu_pct(p: &ProcInfo, prev: &HashMap<u32, (u64, u64)>, delta: f64) -> f64 {
    if let Some(&(pu, ps)) = prev.get(&p.pid) {
        let prev_total = pu + ps;
        let cur_total = p.utime + p.stime;
        if delta > 0.0 {
            return (cur_total.saturating_sub(prev_total) as f64 / delta) * 100.0;
        }
    }
    0.0
}

fn fmt_ram(kb: u64) -> String {
    if kb >= 1_048_576 {
        format!("{:.1} GB", kb as f64 / 1_048_576.0)
    } else if kb >= 1024 {
        format!("{:.1} MB", kb as f64 / 1024.0)
    } else {
        format!("{} KB", kb)
    }
}

fn state_label(c: char) -> &'static str {
    match c {
        'R' => "Running",
        'S' => "Sleep",
        'D' => "Disk I/O",
        'Z' => "Zombie",
        'T' => "Stopped",
        'I' => "Idle",
        _ => "Other",
    }
}

fn state_color(c: char) -> Color32 {
    match c {
        'R' => Color32::from_rgb(80, 230, 130),
        'S' => Color32::from_rgb(100, 160, 255),
        'D' => Color32::from_rgb(230, 200, 80),
        'Z' => Color32::from_rgb(255, 80, 80),
        'T' => Color32::from_rgb(190, 140, 255),
        'I' => Color32::from_rgb(120, 120, 140),
        _ => Color32::from_rgb(90, 90, 110),
    }
}

fn state_dot(c: char) -> &'static str {
    match c {
        'R' => "\u{25CF}",
        'S' => "\u{25CF}",
        'D' => "\u{25CF}",
        'Z' => "\u{25CF}",
        'T' => "\u{25CF}",
        'I' => "\u{25CB}",
        _ => "\u{25CB}",
    }
}

fn cpu_color(pct: f64, ncpu: usize) -> Color32 {
    let max = (ncpu as f64 * 100.0).max(1.0);
    let r = (pct / max).min(1.0);
    if r < 0.15 {
        Color32::from_rgb(100, 200, 130)
    } else if r < 0.4 {
        Color32::from_rgb(160, 220, 100)
    } else if r < 0.65 {
        Color32::from_rgb(240, 200, 60)
    } else if r < 0.85 {
        Color32::from_rgb(255, 140, 50)
    } else {
        Color32::from_rgb(255, 70, 70)
    }
}

fn ram_bar_color(pct: f64) -> Color32 {
    if pct < 50.0 {
        Color32::from_rgb(60, 180, 255)
    } else if pct < 75.0 {
        Color32::from_rgb(240, 200, 60)
    } else {
        Color32::from_rgb(255, 80, 80)
    }
}

fn pill_button(label: &str, active: bool) -> egui::Button<'_> {
    let (bg, fg) = if active {
        (ACCENT, Color32::WHITE)
    } else {
        (Color32::from_rgb(35, 35, 52), TEXT_DIM)
    };
    egui::Button::new(RichText::new(label).size(11.0).color(fg))
        .fill(bg)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(4.0))
}

impl App for ProcMonitorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.last_update.elapsed() >= self.interval {
            self.refresh();
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG).inner_margin(8.0))
            .show(ctx, |ui| {
                self.ui_header(ui);
                ui.add_space(6.0);
                self.ui_toolbar(ui);
                ui.add_space(6.0);
                self.ui_ram_bar(ui);
                ui.add_space(4.0);
                self.ui_table(ui);
                ui.add_space(2.0);
                self.ui_footer(ui);
            });
    }
}

impl ProcMonitorApp {
    fn ui_header(&self, ui: &mut Ui) {
        egui::Frame::none()
            .fill(SURFACE)
            .rounding(Rounding::same(6.0))
            .inner_margin(egui::Margin::same(10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("\u{2699}").size(20.0).color(ACCENT));
                    ui.add_space(4.0);
                    ui.label(RichText::new("SysMonitor").size(18.0).color(Color32::WHITE).strong());
                    ui.add_space(2.0);
                    ui.label(RichText::new("Linux").size(12.0).color(TEXT_DIM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let ram_used = self.ram_used_kb();
                        let pct = if self.total_ram_kb > 0 {
                            ram_used as f64 / self.total_ram_kb as f64 * 100.0
                        } else {
                            0.0
                        };
                        ui.label(
                            RichText::new(format!("CPU Cores: {}", self.num_cpus))
                                .size(11.0)
                                .color(TEXT_DIM),
                        );
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new(format!("{} / {} ({:.0}%)", fmt_ram(ram_used), fmt_ram(self.total_ram_kb), pct))
                                .size(11.0)
                                .color(Color32::from_rgb(160, 210, 255)),
                        );
                    });
                });
            });
    }

    fn ui_toolbar(&mut self, ui: &mut Ui) {
        egui::Frame::none()
            .fill(SURFACE)
            .rounding(Rounding::same(6.0))
            .inner_margin(egui::Margin::symmetric(8.0, 6.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("\u{1F50D}").size(14.0));
                    let search_resp = ui.add_sized(
                        [180.0, 22.0],
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text(RichText::new("Buscar processo...").color(Color32::from_rgb(80, 80, 100)))
                            .text_color(TEXT),
                    );
                    if search_resp.changed() {
                        self.search = self.search.trim().to_string();
                    }

                    ui.add_space(8.0);

                    ui.label(RichText::new("Intervalo:").color(TEXT_DIM).size(11.0));
                    for (lbl, ms) in [("0.5s", 500u64), ("1s", 1000), ("2s", 2000), ("5s", 5000)] {
                        let sel = self.interval == Duration::from_millis(ms);
                        if ui.add(pill_button(lbl, sel)).clicked() {
                            self.interval = Duration::from_millis(ms);
                        }
                    }

                    ui.add_space(8.0);

                    ui.label(RichText::new("Ordenar:").color(TEXT_DIM).size(11.0));
                    for (lbl, col) in [
                        ("PID", SortCol::Pid),
                        ("Nome", SortCol::Name),
                        ("RAM", SortCol::Ram),
                        ("CPU", SortCol::Cpu),
                        ("VMSIZE", SortCol::VmSize),
                        ("Estado", SortCol::State),
                    ] {
                        let act = self.sort_col == col;
                        let arrow = if act {
                            if self.sort_asc { " \u{25B2}" } else { " \u{25BC}" }
                        } else {
                            ""
                        };
                        let text = format!("{}{}", lbl, arrow);
                        if ui.add(pill_button(&text, act)).clicked() {
                            if self.sort_col == col {
                                self.sort_asc = !self.sort_asc;
                            } else {
                                self.sort_col = col;
                                self.sort_asc = matches!(col, SortCol::Pid | SortCol::Name);
                            }
                        }
                    }
                });
            });
    }

    fn ui_ram_bar(&self, ui: &mut Ui) {
        let ram_used = self.ram_used_kb();
        let pct = if self.total_ram_kb > 0 {
            ram_used as f64 / self.total_ram_kb as f64
        } else {
            0.0
        };

        egui::Frame::none()
            .fill(SURFACE)
            .rounding(Rounding::same(4.0))
            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("RAM").size(10.0).color(TEXT_DIM).strong());
                    let bar_w = ui.available_width() - 200.0;
                    let bar_h = 14.0;
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(bar_w, bar_h), egui::Sense::hover());
                    if ui.is_rect_visible(rect) {
                        ui.painter().rect_filled(rect, Rounding::same(3.0), Color32::from_rgb(30, 30, 48));
                        if pct > 0.0 {
                            let fill_w = rect.width() * pct.min(1.0) as f32;
                            let fill_rect = egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + fill_w, rect.max.y));
                            ui.painter().rect_filled(fill_rect, Rounding::same(3.0), ram_bar_color(pct * 100.0));
                        }
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            format!("{:.0}%  {} / {}", pct * 100.0, fmt_ram(ram_used), fmt_ram(self.total_ram_kb)),
                            egui::FontId::proportional(10.0),
                            Color32::WHITE,
                        );
                    }
                    ui.advance_cursor_after_rect(rect);
                    ui.advance_cursor_after_rect(resp.rect);
                    _ = resp;
                });
            });
    }

    fn ui_table(&mut self, ui: &mut Ui) {
        let sorted = self.sorted();
        let row_h = 22.0;
        let cols: [f32; 7] = [62.0, -1.0, 90.0, 90.0, 90.0, 72.0, 62.0];

        egui::Frame::none()
            .fill(SURFACE)
            .rounding(Rounding::same(6.0))
            .inner_margin(egui::Margin::same(4.0))
            .show(ui, |ui| {
                // header
                ui.horizontal(|ui| {
                    ui.style_mut().visuals.override_text_color = Some(Color32::from_rgb(160, 170, 210));
                    let widths = cols;
                    ui.add_sized([widths[0], row_h], |ui: &mut Ui| ui.label(RichText::new("PID").size(10.0).strong()));
                    ui.add_sized([widths[1].max(ui.available_width() * 0.01), row_h], |ui: &mut Ui| ui.label(RichText::new("NOME").size(10.0).strong()));
                    ui.add_sized([widths[2], row_h], |ui: &mut Ui| ui.label(RichText::new("ESTADO").size(10.0).strong()));
                    ui.add_sized([widths[3], row_h], |ui: &mut Ui| ui.label(RichText::new("RAM").size(10.0).strong()));
                    ui.add_sized([widths[4], row_h], |ui: &mut Ui| ui.label(RichText::new("VMSIZE").size(10.0).strong()));
                    ui.add_sized([widths[5], row_h], |ui: &mut Ui| ui.label(RichText::new("CPU%").size(10.0).strong()));
                    ui.label(RichText::new("").size(10.0));
                });

                ui.add_space(1.0);
                let line_rect = ui.available_rect_before_wrap();
                ui.painter().line_segment(
                    [line_rect.left_top(), line_rect.right_top()],
                    Stroke::new(1.0, Color32::from_rgb(50, 50, 75)),
                );
                ui.add_space(2.0);

                egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .show_rows(ui, row_h, sorted.len(), |ui, range| {
                        for i in range {
                            let p = sorted[i];
                            let cpu = self.cpu_vals.get(&p.pid).copied().unwrap_or(0.0);
                            let bg = if i % 2 == 0 { ROW_EVEN } else { ROW_ODD };

                            egui::Frame::none()
                                .fill(bg)
                                .rounding(Rounding::same(2.0))
                                .inner_margin(egui::Margin::symmetric(2.0, 1.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.add_sized([cols[0], row_h - 4.0], |ui: &mut Ui| {
                                            ui.label(RichText::new(format!("{}", p.pid)).size(11.0).monospace().color(TEXT_DIM))
                                        });
                                        ui.add_sized([ui.available_width() * 0.35, row_h - 4.0], |ui: &mut Ui| {
                                            ui.label(RichText::new(truncate_name(&p.name, 30)).size(11.0).color(TEXT))
                                        });
                                        ui.add_sized([cols[2], row_h - 4.0], |ui: &mut Ui| {
                                            ui.label(RichText::new(format!("{} {}", state_dot(p.state), state_label(p.state))).size(10.0).color(state_color(p.state)))
                                        });
                                        ui.add_sized([cols[3], row_h - 4.0], |ui: &mut Ui| {
                                            ui.label(RichText::new(fmt_ram(p.rss_kb)).size(11.0).color(Color32::from_rgb(200, 215, 240)))
                                        });
                                        ui.add_sized([cols[4], row_h - 4.0], |ui: &mut Ui| {
                                            ui.label(RichText::new(fmt_ram(p.vmsize_kb)).size(10.0).color(TEXT_DIM))
                                        });
                                        ui.add_sized([cols[5], row_h - 4.0], |ui: &mut Ui| {
                                            let cc = cpu_color(cpu, self.num_cpus);
                                            let txt = if cpu >= 10.0 {
                                                RichText::new(format!("{:.1}%", cpu)).size(11.0).color(cc).strong()
                                            } else {
                                                RichText::new(format!("{:.1}%", cpu)).size(11.0).color(cc)
                                            };
                                            ui.label(txt)
                                        });
                                    });
                                });
                        }
                    });
            });
    }

    fn ui_footer(&self, ui: &mut Ui) {
        let elapsed = self.last_update.elapsed().as_millis();
        let secs = elapsed / 1000;
        let ms = elapsed % 1000;

        egui::Frame::none()
            .fill(SURFACE)
            .rounding(Rounding::same(4.0))
            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("\u{2234} {} processos", self.procs.len()))
                            .size(10.0)
                            .color(TEXT_DIM),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(format!("CPUs: {}", self.num_cpus))
                            .size(10.0)
                            .color(TEXT_DIM),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(if secs > 0 {
                            format!("Atualizado {}s {}ms atr\u{00E1}s", secs, ms)
                        } else {
                            format!("Atualizado {}ms atr\u{00E1}s", ms)
                        })
                        .size(10.0)
                        .color(TEXT_DIM),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("Intervalo: {}ms", self.interval.as_millis()))
                                .size(10.0)
                                .color(TEXT_DIM),
                        );
                    });
                });
            });
    }
}

fn truncate_name(name: &str, max: usize) -> String {
    if name.len() <= max {
        name.to_string()
    } else {
        format!("{}...", &name[..max - 3])
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([680.0, 400.0])
            .with_title("SysMonitor"),
        ..Default::default()
    };

    eframe::run_native(
        "SysMonitor",
        options,
        Box::new(|cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.window_rounding = Rounding::same(8.0);
            visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(22, 22, 35);
            visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
            visuals.widgets.inactive.bg_fill = Color32::from_rgb(30, 30, 48);
            visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
            visuals.widgets.hovered.bg_fill = Color32::from_rgb(45, 45, 65);
            visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
            visuals.widgets.active.bg_fill = ACCENT_DIM;
            visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
            visuals.selection.bg_fill = ACCENT_DIM;
            visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
            cc.egui_ctx.set_visuals(visuals);
            Ok(Box::new(ProcMonitorApp::new(cc)))
        }),
    )
}
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::{
    egui::{self, Color32, RichText, Stroke, Ui},
    App,
};
use std::collections::HashMap;
use std::fs;
use std::time::{Duration, Instant};

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
        'S' => "Sleeping",
        'D' => "Disk Sleep",
        'Z' => "Zombie",
        'T' => "Stopped",
        'I' => "Idle",
        _ => "Other",
    }
}

fn state_color(c: char) -> Color32 {
    match c {
        'R' => Color32::GREEN,
        'S' => Color32::from_rgb(100, 180, 255),
        'D' => Color32::from_rgb(200, 200, 100),
        'Z' => Color32::RED,
        'T' => Color32::from_rgb(200, 150, 255),
        'I' => Color32::GRAY,
        _ => Color32::DARK_GRAY,
    }
}

fn cpu_color(pct: f64, ncpu: usize) -> Color32 {
    let max = (ncpu as f64 * 100.0).max(1.0);
    let r = (pct / max).min(1.0);
    if r < 0.33 {
        Color32::GREEN
    } else if r < 0.66 {
        Color32::YELLOW
    } else {
        Color32::RED
    }
}

fn hdr(s: &str) -> RichText {
    RichText::new(s).size(11.0).color(Color32::from_rgb(160, 160, 190)).strong()
}

impl App for ProcMonitorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.last_update.elapsed() >= self.interval {
            self.refresh();
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::from_rgb(18, 18, 28)))
            .show(ctx, |ui| {
                self.ui_header(ui);
                ui.add_space(4.0);
                self.ui_toolbar(ui);
                ui.add_space(4.0);
                self.ui_table(ui);
                ui.add_space(2.0);
                self.ui_footer(ui);
            });
    }
}

impl ProcMonitorApp {
    fn ui_header(&self, ui: &mut Ui) {
        ui.heading(RichText::new("SysMonitor").size(17.0).color(Color32::WHITE));
    }

    fn ui_toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("🔍").size(15.0));
            ui.add_sized(
                [200.0, 22.0],
                egui::TextEdit::singleline(&mut self.search).hint_text("Buscar..."),
            );

            ui.separator();

            ui.label(RichText::new("Atualização:").color(Color32::GRAY).size(12.0));
            for (lbl, ms) in [("0.5s", 500u64), ("1s", 1000), ("2s", 2000), ("5s", 5000)] {
                let sel = self.interval == Duration::from_millis(ms);
                let btn = egui::Button::new(RichText::new(lbl).size(11.0))
                    .fill(if sel {
                        Color32::from_rgb(50, 110, 200)
                    } else {
                        Color32::from_rgb(40, 40, 55)
                    })
                    .stroke(Stroke::NONE);
                if ui.add(btn).clicked() {
                    self.interval = Duration::from_millis(ms);
                }
            }

            ui.separator();

            ui.label(RichText::new("Ordenar:").color(Color32::GRAY).size(12.0));
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
                    if self.sort_asc { " ▲" } else { " ▼" }
                } else {
                    ""
                };
                let btn = egui::Button::new(
                    RichText::new(format!("{}{}", lbl, arrow))
                        .size(11.0)
                        .color(if act { Color32::WHITE } else { Color32::GRAY }),
                )
                .fill(if act {
                    Color32::from_rgb(50, 110, 200)
                } else {
                    Color32::from_rgb(35, 35, 50)
                })
                .stroke(Stroke::NONE);
                if ui.add(btn).clicked() {
                    if self.sort_col == col {
                        self.sort_asc = !self.sort_asc;
                    } else {
                        self.sort_col = col;
                        self.sort_asc = matches!(col, SortCol::Pid | SortCol::Name);
                    }
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ram_used: u64 = self.procs.iter().map(|p| p.rss_kb).sum();
                let pct = if self.total_ram_kb > 0 {
                    ram_used as f64 / self.total_ram_kb as f64 * 100.0
                } else {
                    0.0
                };
                ui.label(
                    RichText::new(format!(
                        "{} / {} ({:.0}%)",
                        fmt_ram(ram_used),
                        fmt_ram(self.total_ram_kb),
                        pct
                    ))
                    .size(12.0)
                    .color(Color32::from_rgb(180, 220, 255)),
                );
            });
        });
    }

    fn ui_table(&mut self, ui: &mut Ui) {
        let sorted = self.sorted();

        ui.horizontal(|ui| {
            ui.set_min_width(ui.available_width());
            self.header_row(ui);
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink([false, true])
            .show_rows(ui, 20.0, sorted.len(), |ui, range| {
                for i in range {
                    let p = sorted[i];
                    let cpu = self.cpu_vals.get(&p.pid).copied().unwrap_or(0.0);

                    ui.horizontal(|ui| {
                        ui.set_min_width(ui.available_width());
                        self.data_row(ui, p, cpu);
                    });

                    if i < sorted.len() - 1 {
                        ui.separator();
                    }
                }
            });
    }

    fn header_row(&self, ui: &mut Ui) {
        ui.set_min_width(55.0);
        ui.label(hdr("PID"));
        ui.set_min_width(200.0);
        ui.label(hdr("Nome"));
        ui.set_min_width(90.0);
        ui.label(hdr("Estado"));
        ui.set_min_width(95.0);
        ui.label(hdr("RAM"));
        ui.set_min_width(100.0);
        ui.label(hdr("VMSIZE"));
        ui.set_min_width(75.0);
        ui.label(hdr("CPU%"));
    }

    fn data_row(&self, ui: &mut Ui, p: &ProcInfo, cpu: f64) {
        ui.set_min_width(55.0);
        ui.label(RichText::new(format!("{}", p.pid)).size(11.0).color(Color32::GRAY));
        ui.set_min_width(200.0);
        ui.label(RichText::new(&p.name).size(11.0).color(Color32::WHITE));
        ui.set_min_width(90.0);
        ui.label(RichText::new(state_label(p.state)).size(10.0).color(state_color(p.state)));
        ui.set_min_width(95.0);
        ui.label(RichText::new(fmt_ram(p.rss_kb)).size(11.0).color(Color32::from_rgb(210, 210, 210)));
        ui.set_min_width(100.0);
        ui.label(RichText::new(fmt_ram(p.vmsize_kb)).size(10.0).color(Color32::GRAY));
        ui.set_min_width(75.0);
        ui.label(
            RichText::new(format!("{:.1}%", cpu))
                .size(11.0)
                .color(cpu_color(cpu, self.num_cpus)),
        );
    }

    fn ui_footer(&self, ui: &mut Ui) {
        let elapsed = self.last_update.elapsed().as_millis();
        ui.label(
            RichText::new(format!(
                "Processos: {}  |  CPUs: {}  |  Última atualização: {}ms atrás",
                self.procs.len(),
                self.num_cpus,
                elapsed
            ))
            .size(10.0)
            .color(Color32::GRAY),
        );
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 600.0])
            .with_min_inner_size([650.0, 380.0])
            .with_title("SysMonitor — Linux"),
        ..Default::default()
    };

    eframe::run_native(
        "SysMonitor",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(ProcMonitorApp::new(cc)))
        }),
    )
}

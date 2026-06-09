use eframe::egui::{self, Color32, RichText};
use egui_plot::{Line, Plot, PlotPoints};
use std::collections::VecDeque;
use sysinfo::{Pid, Process, ProcessesToUpdate, System, Users};

const MAX_HISTORY: usize = 120;
const REFRESH_MS: u64 = 500;

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

struct ProcRow {
    pid: Pid,
    user: String,
    name: String,
    cpu: f32,
    mem_bytes: u64,
    mem_pct: f32,
    status: String,
}

struct SysMonitor {
    sys: System,
    users: Users,
    tab: Tab,
    sort_col: SortCol,
    sort_ascending: bool,
    filter: String,
    processes: Vec<ProcRow>,
    cpu_history: VecDeque<f64>,
    mem_history: VecDeque<f64>,
    last_refresh: std::time::Instant,
}

impl SysMonitor {
    fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        let users = Users::new_with_refreshed_list();
        let mut app = Self {
            sys,
            users,
            tab: Tab::Overview,
            sort_col: SortCol::Cpu,
            sort_ascending: false,
            filter: String::new(),
            processes: Vec::new(),
            cpu_history: VecDeque::with_capacity(MAX_HISTORY),
            mem_history: VecDeque::with_capacity(MAX_HISTORY),
            last_refresh: std::time::Instant::now(),
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
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

        self.update_process_list();
        self.last_refresh = std::time::Instant::now();
    }

    fn update_process_list(&mut self) {
        let total_mem = self.sys.total_memory() as f32;
        self.processes.clear();
        let filter_lower = self.filter.to_lowercase();

        for (pid, proc_) in self.sys.processes() {
            let name = proc_.name().to_string_lossy().to_string();
            if !filter_lower.is_empty() && !name.to_lowercase().contains(&filter_lower) {
                continue;
            }
            let user = self.get_user_name(proc_);
            let cpu = proc_.cpu_usage();
            let mem_bytes = proc_.memory();
            let mem_pct = if total_mem > 0.0 {
                (mem_bytes as f32 / total_mem) * 100.0
            } else {
                0.0
            };
            let status = format!("{:?}", proc_.status());
            self.processes.push(ProcRow {
                pid: *pid,
                user,
                name,
                cpu,
                mem_bytes,
                mem_pct,
                status,
            });
        }

        self.processes.sort_by(|a, b| {
            let ord = match self.sort_col {
                SortCol::Pid => a.pid.cmp(&b.pid),
                SortCol::User => a.user.cmp(&b.user),
                SortCol::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortCol::Cpu => a.cpu.partial_cmp(&b.cpu).unwrap_or(std::cmp::Ordering::Equal),
                SortCol::MemBytes => a.mem_bytes.cmp(&b.mem_bytes),
                SortCol::MemPct => a.mem_pct.partial_cmp(&b.mem_pct).unwrap_or(std::cmp::Ordering::Equal),
                SortCol::Status => a.status.cmp(&b.status),
            };
            if self.sort_ascending { ord } else { ord.reverse() }
        });
    }

    fn get_user_name(&self, proc_: &Process) -> String {
        if let Some(uid) = proc_.user_id() {
            for user in self.users.list() {
                if user.id() == uid {
                    return user.name().to_string();
                }
            }
        }
        "-".to_string()
    }

    fn sort_button(ui: &mut egui::Ui, label: &str, col: SortCol, current: &mut SortCol, asc: &mut bool) {
        let active = *current == col;
        let arrow = if active { if *asc { " ▲" } else { " ▼" } } else { "" };
        let text = format!("{}{}", label, arrow);
        if ui.selectable_label(active, &text).clicked() {
            if active {
                *asc = !*asc;
            } else {
                *current = col;
                *asc = false;
            }
        }
    }
}

impl eframe::App for SysMonitor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.last_refresh.elapsed().as_millis() >= REFRESH_MS as u128 {
            self.refresh();
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(REFRESH_MS));

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let hostname = System::host_name().unwrap_or_else(|| "?".into());
                let kernel = System::kernel_version().unwrap_or_else(|| "?".into());
                let uptime = System::uptime();
                let d = uptime / 86400;
                let h = (uptime % 86400) / 3600;
                let m = (uptime % 3600) / 60;
                let load = System::load_average();

                ui.label(RichText::new(&hostname).color(Color32::from_rgb(0, 200, 255)).strong());
                ui.separator();
                ui.label(format!("Kernel: {}", kernel));
                ui.separator();
                ui.label(format!("Uptime: {}d {}h {}m", d, h, m));
                ui.separator();
                ui.label(format!("Load: {:.2} {:.2} {:.2}", load.one, load.five, load.fifteen));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{} processos", self.processes.len())).color(Color32::YELLOW));
                });
            });
        });

        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Overview, "📊 Visão Geral");
                ui.selectable_value(&mut self.tab, Tab::Processes, "📋 Processos");
            });
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
            ui.horizontal(|ui| {
                ui.set_width(ui.available_width());
                let color = if cpu_global > 90.0 {
                    Color32::RED
                } else if cpu_global > 60.0 {
                    Color32::YELLOW
                } else {
                    Color32::GREEN
                };
                let bar = egui::ProgressBar::new(cpu_global / 100.0)
                    .text(format!("Total: {:.1}%", cpu_global))
                    .fill(color)
                    .animate(true);
                ui.add(bar);
            });

            ui.add_space(4.0);

            let cpus = self.sys.cpus();
            let num = cpus.len();
            let cols = if num <= 8 { 2 } else if num <= 16 { 4 } else { 8 };
            egui::Grid::new("cpu_cores")
                .num_columns(cols)
                .spacing([8.0, 2.0])
                .show(ui, |ui| {
                    for (i, cpu) in cpus.iter().enumerate() {
                        let usage = cpu.cpu_usage();
                        let color = if usage > 90.0 {
                            Color32::RED
                        } else if usage > 60.0 {
                            Color32::YELLOW
                        } else {
                            Color32::from_rgb(80, 200, 80)
                        };
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("C{}", i)).monospace().size(10.0));
                            let bar = egui::ProgressBar::new(usage / 100.0)
                                .desired_width(80.0)
                                .fill(color)
                                .text(format!("{:.0}%", usage));
                            ui.add(bar);
                        });
                        if (i + 1) % cols == 0 {
                            ui.end_row();
                        }
                    }
                });

            ui.add_space(8.0);

            ui.label(RichText::new("Histórico CPU (%)").strong());
            let cpu_pts: PlotPoints = self
                .cpu_history
                .iter()
                .enumerate()
                .map(|(i, &v)| [i as f64, v])
                .collect();
            Plot::new("cpu_plot")
                .height(120.0)
                .view_aspect(4.0)
                .include_y(0.0)
                .include_y(100.0)
                .show_x(false)
                .show(ui, |plot_ui| {
                    plot_ui.line(Line::new(cpu_pts).color(Color32::from_rgb(80, 220, 80)).width(2.0));
                });

            ui.separator();

            ui.heading(RichText::new("Memória RAM").color(Color32::from_rgb(80, 180, 255)));
            let mem_pct = if total_mem > 0 {
                (used_mem as f64 / total_mem as f64) * 100.0
            } else {
                0.0
            };
            let mcolor = if mem_pct > 90.0 {
                Color32::RED
            } else if mem_pct > 70.0 {
                Color32::YELLOW
            } else {
                Color32::from_rgb(80, 180, 255)
            };
            ui.add(
                egui::ProgressBar::new((mem_pct / 100.0) as f32)
                    .text(format!(
                        "{} / {} ({:.1}%)",
                        format_bytes(used_mem),
                        format_bytes(total_mem),
                        mem_pct
                    ))
                    .fill(mcolor)
                    .animate(true),
            );

            egui::Grid::new("mem_details").num_columns(2).show(ui, |ui| {
                ui.label("Disponível:");
                ui.label(RichText::new(format_bytes(avail_mem)).color(Color32::GREEN));
                ui.end_row();
                ui.label("Livre:");
                ui.label(format_bytes(free_mem));
                ui.end_row();
            });

            ui.add_space(4.0);
            ui.label(RichText::new("Histórico Memória (%)").strong());
            let mem_pts: PlotPoints = self
                .mem_history
                .iter()
                .enumerate()
                .map(|(i, &v)| [i as f64, v])
                .collect();
            Plot::new("mem_plot")
                .height(120.0)
                .view_aspect(4.0)
                .include_y(0.0)
                .include_y(100.0)
                .show_x(false)
                .show(ui, |plot_ui| {
                    plot_ui.line(Line::new(mem_pts).color(Color32::from_rgb(80, 180, 255)).width(2.0));
                });

            if total_swap > 0 {
                ui.separator();
                ui.heading(RichText::new("Swap").color(Color32::from_rgb(200, 100, 220)));
                let spct = (used_swap as f64 / total_swap as f64) * 100.0;
                ui.add(
                    egui::ProgressBar::new((spct / 100.0) as f32)
                        .text(format!(
                            "{} / {} ({:.1}%)",
                            format_bytes(used_swap),
                            format_bytes(total_swap),
                            spct
                        ))
                        .fill(Color32::from_rgb(200, 100, 220))
                        .animate(true),
                );
            }

            ui.separator();
            ui.heading(RichText::new("Top 10 - CPU").color(Color32::YELLOW));
            let mut top_cpu: Vec<&ProcRow> = self.processes.iter().collect();
            top_cpu.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap());
            top_cpu.truncate(10);
            egui::Grid::new("top_cpu_grid").striped(true).show(ui, |ui| {
                ui.label(RichText::new("PID").strong());
                ui.label(RichText::new("Nome").strong());
                ui.label(RichText::new("CPU%").strong());
                ui.label(RichText::new("Memória").strong());
                ui.end_row();
                for p in &top_cpu {
                    ui.label(p.pid.to_string());
                    ui.label(&p.name);
                    let c = if p.cpu > 80.0 { Color32::RED } else if p.cpu > 40.0 { Color32::YELLOW } else { Color32::WHITE };
                    ui.label(RichText::new(format!("{:.1}%", p.cpu)).color(c));
                    ui.label(format_bytes(p.mem_bytes));
                    ui.end_row();
                }
            });

            ui.add_space(8.0);
            ui.heading(RichText::new("Top 10 - Memória").color(Color32::from_rgb(80, 180, 255)));
            let mut top_mem: Vec<&ProcRow> = self.processes.iter().collect();
            top_mem.sort_by(|a, b| b.mem_bytes.cmp(&a.mem_bytes));
            top_mem.truncate(10);
            egui::Grid::new("top_mem_grid").striped(true).show(ui, |ui| {
                ui.label(RichText::new("PID").strong());
                ui.label(RichText::new("Nome").strong());
                ui.label(RichText::new("Memória").strong());
                ui.label(RichText::new("MEM%").strong());
                ui.end_row();
                for p in &top_mem {
                    ui.label(p.pid.to_string());
                    ui.label(&p.name);
                    ui.label(RichText::new(format_bytes(p.mem_bytes)).color(Color32::from_rgb(80, 180, 255)));
                    ui.label(format!("{:.1}%", p.mem_pct));
                    ui.end_row();
                }
            });
        });
    }

    fn draw_processes(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Filtrar:");
            let resp = ui.text_edit_singleline(&mut self.filter);
            if resp.changed() {
                self.update_process_list();
            }
            if ui.button("Limpar").clicked() {
                self.filter.clear();
                self.update_process_list();
            }
            ui.separator();
            ui.label(RichText::new(format!("{} processos", self.processes.len())).color(Color32::YELLOW));
        });

        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("proc_table")
                .striped(true)
                .num_columns(8)
                .min_col_width(40.0)
                .show(ui, |ui| {
                    Self::sort_button(ui, "PID", SortCol::Pid, &mut self.sort_col, &mut self.sort_ascending);
                    Self::sort_button(ui, "Usuário", SortCol::User, &mut self.sort_col, &mut self.sort_ascending);
                    Self::sort_button(ui, "Nome", SortCol::Name, &mut self.sort_col, &mut self.sort_ascending);
                    Self::sort_button(ui, "CPU%", SortCol::Cpu, &mut self.sort_col, &mut self.sort_ascending);
                    Self::sort_button(ui, "Memória", SortCol::MemBytes, &mut self.sort_col, &mut self.sort_ascending);
                    Self::sort_button(ui, "MEM%", SortCol::MemPct, &mut self.sort_col, &mut self.sort_ascending);
                    Self::sort_button(ui, "Status", SortCol::Status, &mut self.sort_col, &mut self.sort_ascending);
                    ui.label(RichText::new("Ação").strong());
                    ui.end_row();

                    let pids_to_kill: std::cell::RefCell<Vec<Pid>> = std::cell::RefCell::new(Vec::new());

                    for p in &self.processes {
                        ui.label(p.pid.to_string());
                        ui.label(&p.user);
                        ui.label(&p.name);
                        let cc = if p.cpu > 80.0 { Color32::RED } else if p.cpu > 40.0 { Color32::YELLOW } else { Color32::WHITE };
                        ui.label(RichText::new(format!("{:.1}", p.cpu)).color(cc));
                        ui.label(format_bytes(p.mem_bytes));
                        let mc = if p.mem_pct > 50.0 { Color32::RED } else if p.mem_pct > 20.0 { Color32::YELLOW } else { Color32::WHITE };
                        ui.label(RichText::new(format!("{:.1}%", p.mem_pct)).color(mc));
                        ui.label(&p.status);
                        if ui.button("✕").on_hover_text("Matar processo").clicked() {
                            pids_to_kill.borrow_mut().push(p.pid);
                        }
                        ui.end_row();
                    }

                    for pid in pids_to_kill.into_inner() {
                        if let Some(proc_) = self.sys.process(pid) {
                            proc_.kill();
                        }
                    }
                });
        });
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SysMonitor")
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size([700.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "SysMonitor",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_theme(egui::Theme::Dark);
            Ok(Box::new(SysMonitor::new()))
        }),
    )
}

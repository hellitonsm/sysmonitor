# SysMonitor

Monitor de sistema visual para Linux escrito em Rust com GUI (egui).

![Preview](assets/sysmonitor.png)

## Funcionalidades

- **Visão Geral**: CPU por núcleo, gráficos em tempo real de CPU e memória, top processos por recurso
- **Processos**: Lista completa de todos os processos do sistema com PID, usuário, nome, CPU%, memória, MEM% e status
- **Filtro e ordenação**: Filtre por nome, ordene por qualquer coluna clicando no cabeçalho
- **Matar processo**: Botão para encerrar processos selecionados
- **Swap e detalhes**: Uso de swap, memória disponível, total, uptime, load average

## Captura de Tela

```
CPU ┌─────────────────────────────────────────────┐
    │ C0 45%  C1 23%  C2 67%  C3 12%  Total: 37%│
    └─────────────────────────────────────────────┘
Mem ┌─────────────────────────────────────────────┐
    │ 4.2 GB / 7.7 GB (55%)   Disp: 3.9 GB       │
    └─────────────────────────────────────────────┘
```

## Instalação

### Requerimentos

- Rust (edition 2021+) — [rustup.rs](https://rustup.rs)
- Sistema Linux com X11/Wayland

### Compilar e instalar

```bash
make install
```

Ou manualmente:

```bash
cargo build --release
cp target/release/sysmonitor ~/.local/bin/
cp assets/sysmonitor.png ~/.local/share/icons/
cp sysmonitor.desktop ~/.local/share/applications/
update-desktop-database ~/.local/share/applications/
```

### Executar

```bash
sysmonitor
```

Ou pelo menu de aplicativos → SysMonitor.

## Atalhos

| Tecla       | Ação            |
|-------------|-----------------|
| Navegação   | Scroll / clique |
| Matar       | Botão ✕ na tabela |
| Filtro      | Campo de texto  |
| Ordenar     | Clique no cabeçalho |

## Tecnologias

- [egui](https://github.com/emilk/egui) — GUI imediata em Rust
- [sysinfo](https://github.com/GuillaumeGomez/sysinfo) — Informações do sistema
- Rust — Linguagem de programação

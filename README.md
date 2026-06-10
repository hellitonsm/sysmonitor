# SysMonitor

Monitor de sistema visual para Linux escrito em Rust com GUI (egui).

![Preview](assets/sysmonitor.png)

## Funcionalidades

- **Tabela de processos**: PID, nome, estado, RAM, VM, CPU% com cores por carga
- **Busca e ordenação**: Filtre por nome ou PID, ordene por qualquer coluna
- **Matar processo**: Botão ✕ para encerrar via `kill`
- **Barra de RAM**: Gráfico de uso com percentual
- **Intervalo ajustável**: 0.5s / 1s / 2s / 5s
- **Modo lite**: Versão mais leve via `--lite` (sem dependência de ícones, visual simplificado)

## Modos

| Modo   | Comando                    | Tema                    | Tamanho   |
|--------|----------------------------|-------------------------|-----------|
| Full   | `sysmonitor`               | Indigo escuro moderno   | 900×600   |
| Lite   | `sysmonitor --lite`        | Azul clássico           | 760×480   |

## Instalação

### Requerimentos

- Rust (edition 2021+) — [rustup.rs](https://rustup.rs)
- Sistema Linux com X11/Wayland

### Compilar e instalar (modo full)

```bash
make install
```

### Compilar e instalar (modo lite)

```bash
make install-lite
```

Instala `sysmonitor` + cria o wrapper `sysmonitor-lite` que já passa `--lite`.

### Manualmente

```bash
cargo build --release
cp target/release/sysmonitor ~/.local/bin/
cp assets/sysmonitor.png ~/.local/share/icons/
cp sysmonitor.desktop ~/.local/share/applications/
update-desktop-database ~/.local/share/applications/
```

### Executar

```bash
sysmonitor          # modo full
sysmonitor --lite   # modo lite
sysmonitor-lite     # modo lite (se instalado via make install-lite)
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
- Leitura direta de `/proc` — sem dependência de sysinfo
- Rust — Linguagem de programação

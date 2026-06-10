PREFIX ?= $(HOME)/.local

BINARY_NAME = sysmonitor
BINARY_PATH = target/release/$(BINARY_NAME)
DESKTOP_FILE = sysmonitor.desktop
ICON_FILE = assets/sysmonitor.png

.PHONY: all build install uninstall clean

all: build

build:
	cargo build --release

install: build
	@mkdir -p $(PREFIX)/bin $(PREFIX)/share/icons/hicolor/256x256/apps $(PREFIX)/share/applications
	cp $(BINARY_PATH) $(PREFIX)/bin/
	cp $(ICON_FILE) $(PREFIX)/share/icons/hicolor/256x256/apps/$(BINARY_NAME).png
	cp $(DESKTOP_FILE) $(PREFIX)/share/applications/
	@chmod +x $(PREFIX)/bin/$(BINARY_NAME)
	@-update-desktop-database $(PREFIX)/share/applications/ 2>/dev/null || true
	@-gtk-update-icon-cache $(PREFIX)/share/icons/hicolor/ 2>/dev/null || true
	@echo "Instalado em $(PREFIX)"

uninstall:
	rm -f $(PREFIX)/bin/$(BINARY_NAME)
	rm -f $(PREFIX)/bin/$(BINARY_NAME)-lite
	rm -f $(PREFIX)/share/icons/hicolor/256x256/apps/$(BINARY_NAME).png
	rm -f $(PREFIX)/share/applications/$(DESKTOP_FILE)
	rm -f $(PREFIX)/share/applications/$(BINARY_NAME)-lite.desktop
	@-update-desktop-database $(PREFIX)/share/applications/ 2>/dev/null || true
	@echo "Removido de $(PREFIX)"

install-lite: build
	@mkdir -p $(PREFIX)/bin $(PREFIX)/share/applications
	cp $(BINARY_PATH) $(PREFIX)/bin/
	@chmod +x $(PREFIX)/bin/$(BINARY_NAME)
	printf '#!/bin/sh\nexec %s --lite "$$@"\n' "$(PREFIX)/bin/$(BINARY_NAME)" > $(PREFIX)/bin/$(BINARY_NAME)-lite
	chmod +x $(PREFIX)/bin/$(BINARY_NAME)-lite
	printf '[Desktop Entry]\nName=SysMonitor (Lite)\nComment=System resource monitor (lite)\nExec=%s --lite\nIcon=%s\nTerminal=false\nType=Application\nCategories=System;Monitor;\nStartupNotify=true\n' "$(PREFIX)/bin/$(BINARY_NAME)" "$(PREFIX)/share/icons/hicolor/256x256/apps/$(BINARY_NAME).png" > $(PREFIX)/share/applications/$(BINARY_NAME)-lite.desktop
	@-update-desktop-database $(PREFIX)/share/applications/ 2>/dev/null || true
	@echo "Instalado em $(PREFIX) (modo lite disponível como $(BINARY_NAME)-lite)"

install-system:
	sudo make PREFIX=/usr/local install

uninstall-system:
	sudo make PREFIX=/usr/local uninstall

clean:
	cargo clean

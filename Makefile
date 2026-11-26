NAME        := lapse
VERSION     := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
CARGO       ?= cargo
CARGOFLAGS  ?= --release
INSTALL     ?= install
TARGET_DIR  ?= target/release

# rustup's musl target links statically by default, and a static binary can't
# load the graphics libraries the window needs.
ifneq ($(findstring musl,$(shell rustc -vV 2>/dev/null | sed -n 's/^host: //p')),)
  export RUSTFLAGS += -C target-feature=-crt-static
endif

ifeq ($(origin PREFIX), undefined)
  ifneq ($(strip $(DESTDIR)),)
    PREFIX := /usr
  else ifeq ($(shell id -u), 0)
    PREFIX := /usr/local
  else
    PREFIX := $(HOME)/.local
  endif
endif

BINDIR      := $(DESTDIR)$(PREFIX)/bin
DATADIR     := $(DESTDIR)$(PREFIX)/share
APPSDIR     := $(DATADIR)/applications
ICONSDIR    := $(DATADIR)/icons/hicolor/512x512/apps
SOUNDSDIR   := $(DATADIR)/sounds/$(NAME)
LICENSEDIR  := $(DATADIR)/licenses/$(NAME)

.PHONY: all build install uninstall clean

all: build

build:
	$(CARGO) build $(CARGOFLAGS)

install:
	@BIN_SRC="$(TARGET_DIR)/Lapse"; \
	if [ ! -f "$$BIN_SRC" ] && [ -f "target/x86_64-unknown-linux-gnu/release/Lapse" ]; then \
		BIN_SRC="target/x86_64-unknown-linux-gnu/release/Lapse"; \
	elif [ ! -f "$$BIN_SRC" ] && [ -f "target/x86_64-unknown-linux-musl/release/Lapse" ]; then \
		BIN_SRC="target/x86_64-unknown-linux-musl/release/Lapse"; \
	fi; \
	if [ ! -f "$$BIN_SRC" ]; then \
		echo "Binary not found, building release first..."; \
		$(CARGO) build $(CARGOFLAGS); \
		BIN_SRC="$(TARGET_DIR)/Lapse"; \
	fi; \
	$(INSTALL) -Dm755 "$$BIN_SRC" "$(BINDIR)/lapse"; \
	ln -sf lapse "$(BINDIR)/Lapse"
	$(INSTALL) -Dm644 assets/icon.png "$(ICONSDIR)/lapse.png"
	$(INSTALL) -Dm644 assets/lapse.svg "$(DATADIR)/icons/hicolor/scalable/apps/lapse.svg"
	$(INSTALL) -Dm644 assets/lapse-symbolic.svg "$(DATADIR)/icons/hicolor/symbolic/apps/lapse-symbolic.svg"
	$(INSTALL) -Dm644 assets/shutter.ogg "$(SOUNDSDIR)/shutter.ogg"
	$(INSTALL) -Dm644 LICENSE "$(LICENSEDIR)/LICENSE"
	@mkdir -p "$(APPSDIR)"
	@printf "[Desktop Entry]\nName=Lapse\nComment=Modern Gameplay Replay & Lossless Clipping Studio\nExec=%s/bin/lapse --gui\nIcon=lapse\nTerminal=false\nType=Application\nCategories=AudioVideo;Video;Recorder;\nKeywords=replay;record;screen;clip;gaming;gpu;\nStartupWMClass=lapse\n" "$(PREFIX)" > "$(APPSDIR)/lapse.desktop"
	@chmod 644 "$(APPSDIR)/lapse.desktop"
	@if [ -z "$(DESTDIR)" ]; then \
		command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$(APPSDIR)" 2>/dev/null || true; \
		command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -f -t "$(DATADIR)/icons/hicolor" 2>/dev/null || true; \
	fi
	@echo "Installed Lapse $(VERSION) to $(BINDIR)/lapse"

uninstall:
	rm -f "$(BINDIR)/lapse" "$(BINDIR)/Lapse"
	rm -f "$(APPSDIR)/lapse.desktop"
	rm -f "$(ICONSDIR)/lapse.png"
	rm -f "$(DATADIR)/icons/hicolor/scalable/apps/lapse.svg" "$(DATADIR)/icons/hicolor/symbolic/apps/lapse-symbolic.svg"
	rm -rf "$(SOUNDSDIR)" "$(LICENSEDIR)"
	@echo "Uninstalled Lapse from $(PREFIX)"

clean:
	$(CARGO) clean
	rm -rf dist

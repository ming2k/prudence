PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
MANDIR ?= $(PREFIX)/share/man/man1
BASHCOMPDIR ?= $(PREFIX)/share/bash-completion/completions
ZSHCOMPDIR ?= $(PREFIX)/share/zsh/site-functions
FISHCOMPDIR ?= $(PREFIX)/share/fish/vendor_completions.d

USER_PREFIX ?= $(HOME)/.local
USER_BINDIR ?= $(USER_PREFIX)/bin
USER_MANDIR ?= $(USER_PREFIX)/share/man/man1
USER_BASHCOMPDIR ?= $(USER_PREFIX)/share/bash-completion/completions
USER_ZSHCOMPDIR ?= $(USER_PREFIX)/share/zsh/site-functions
USER_FISHCOMPDIR ?= $(HOME)/.config/fish/completions

.PHONY: all build check test install install-user uninstall clean

all: build

build:
	cargo build --release

check:
	cargo check

test:
	cargo test

install: build
	install -d "$(DESTDIR)$(BINDIR)"
	install -m 755 target/release/prudence "$(DESTDIR)$(BINDIR)/prudence"
	ln -sfn prudence "$(DESTDIR)$(BINDIR)/trash"
	install -d "$(DESTDIR)$(MANDIR)"
	install -m 644 man/man1/prudence.1 "$(DESTDIR)$(MANDIR)/prudence.1"
	install -d "$(DESTDIR)$(BASHCOMPDIR)"
	install -m 644 completions/prudence.bash "$(DESTDIR)$(BASHCOMPDIR)/prudence"
	install -d "$(DESTDIR)$(ZSHCOMPDIR)"
	install -m 644 completions/_prudence "$(DESTDIR)$(ZSHCOMPDIR)/_prudence"
	install -d "$(DESTDIR)$(FISHCOMPDIR)"
	install -m 644 completions/prudence.fish "$(DESTDIR)$(FISHCOMPDIR)/prudence.fish"

install-user: build
	install -d "$(USER_BINDIR)"
	install -m 755 target/release/prudence "$(USER_BINDIR)/prudence"
	ln -sfn prudence "$(USER_BINDIR)/trash"
	install -d "$(USER_MANDIR)"
	install -m 644 man/man1/prudence.1 "$(USER_MANDIR)/prudence.1"
	install -d "$(USER_BASHCOMPDIR)"
	install -m 644 completions/prudence.bash "$(USER_BASHCOMPDIR)/prudence"
	install -d "$(USER_ZSHCOMPDIR)"
	install -m 644 completions/_prudence "$(USER_ZSHCOMPDIR)/_prudence"
	install -d "$(USER_FISHCOMPDIR)"
	install -m 644 completions/prudence.fish "$(USER_FISHCOMPDIR)/prudence.fish"

uninstall:
	rm -f "$(DESTDIR)$(BINDIR)/prudence"
	rm -f "$(DESTDIR)$(BINDIR)/trash"
	rm -f "$(DESTDIR)$(MANDIR)/prudence.1"
	rm -f "$(DESTDIR)$(BASHCOMPDIR)/prudence"
	rm -f "$(DESTDIR)$(ZSHCOMPDIR)/_prudence"
	rm -f "$(DESTDIR)$(FISHCOMPDIR)/prudence.fish"

clean:
	cargo clean

# User install by default; rofi needs `-plugin-path ~/.local/lib/rofi` to find
# it there. For a system-wide install into rofi's own plugin directory:
#   sudo make install PLUGIN_DIR=$(pkg-config --variable=pluginsdir rofi)
PLUGIN_DIR ?= $(HOME)/.local/lib/rofi

build:
	cargo build --release

install: build
	install -Dm644 target/release/libwebsearch.so $(DESTDIR)$(PLUGIN_DIR)/websearch.so

uninstall:
	rm -f $(DESTDIR)$(PLUGIN_DIR)/websearch.so

test:
	cargo test

clean:
	cargo clean

.PHONY: build install uninstall test clean

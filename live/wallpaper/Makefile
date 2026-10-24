CC ?= gcc
CFLAGS ?= -O2 -Wall -Wextra
PKG_CONFIG ?= pkg-config

DEPS = wayland-client cairo gdk-pixbuf-2.0
DEP_CFLAGS = $(shell $(PKG_CONFIG) --cflags $(DEPS))
DEP_LIBS = $(shell $(PKG_CONFIG) --libs $(DEPS)) -lm

TARGET = infynitae-wallpaper
SOURCES = infynitae-wallpaper.c wlr-layer-shell-protocol.c xdg-shell-protocol.c

all: $(TARGET)

$(TARGET): $(SOURCES)
	$(CC) $(CFLAGS) $(DEP_CFLAGS) -o $@ $(SOURCES) $(DEP_LIBS)

clean:
	rm -f $(TARGET)

install: $(TARGET)
	install -d $(DESTDIR)/usr/local/bin
	install -m 755 $(TARGET) $(DESTDIR)/usr/local/bin/$(TARGET)

.PHONY: all clean install

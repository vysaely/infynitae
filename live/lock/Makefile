CC ?= gcc
CFLAGS ?= -O2 -Wall -Wextra
PKG_CONFIG ?= pkg-config

DEPS = wayland-client cairo pangocairo xkbcommon pam
DEP_CFLAGS = $(shell $(PKG_CONFIG) --cflags $(DEPS))
DEP_LIBS = $(shell $(PKG_CONFIG) --libs $(DEPS)) -lm -lpthread

TARGET = infynitae-lock
SOURCES = infynitae-lock.c wlr-layer-shell-protocol.c xdg-shell-protocol.c

all: $(TARGET)

$(TARGET): $(SOURCES)
	$(CC) $(CFLAGS) $(DEP_CFLAGS) -o $@ $(SOURCES) $(DEP_LIBS)

clean:
	rm -f $(TARGET)

install: $(TARGET)
	install -d $(DESTDIR)/usr/local/bin
	install -m 755 $(TARGET) $(DESTDIR)/usr/local/bin/$(TARGET)
	install -d $(DESTDIR)/etc/pam.d
	install -m 644 infynitae.pam $(DESTDIR)/etc/pam.d/infynitae

.PHONY: all clean install

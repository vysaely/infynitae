CC ?= gcc
PKG_CONFIG ?= pkg-config
CFLAGS ?= -O2 -Wall -Wextra -Werror
WL_CFLAGS := $(shell $(PKG_CONFIG) --cflags wayland-client)
WL_LIBS := $(shell $(PKG_CONFIG) --libs wayland-client)

SRCS := frame-probe.c xdg-shell-protocol.c presentation-time-protocol.c

.PHONY: all test clean

all: frame-probe

frame-probe: $(SRCS) probe_stats.h xdg-shell-protocol.h presentation-time-protocol.h
	$(CC) $(CFLAGS) $(WL_CFLAGS) $(SRCS) -o $@ $(WL_LIBS)

test: test-stats
	./test-stats

test-stats: test.c probe_stats.h
	$(CC) $(CFLAGS) test.c -o $@ -lm

clean:
	rm -f frame-probe test-stats

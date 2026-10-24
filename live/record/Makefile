CC ?= gcc
CFLAGS ?= -O3 -Wall -Wextra -pthread $(shell pkg-config --cflags wayland-client)
LDFLAGS ?= $(shell pkg-config --libs wayland-client) -lrt

all: infynitae-record-stream

infynitae-record-stream: infynitae-record-stream.o wlr-screencopy-protocol.o
	$(CC) $(CFLAGS) -o $@ $^ $(LDFLAGS)

infynitae-record-stream.o: infynitae-record-stream.c wlr-screencopy-protocol.h
	$(CC) $(CFLAGS) -c $< -o $@

wlr-screencopy-protocol.o: wlr-screencopy-protocol.c wlr-screencopy-protocol.h
	$(CC) $(CFLAGS) -c $< -o $@

clean:
	rm -f *.o infynitae-record-stream

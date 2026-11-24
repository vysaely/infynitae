import sys, os, re, json
exec(open("/home/tester/g/feat.py").read().split("which = sys.argv[1:]")[0])
# Helpers the VM edge scripts share (exec'd by each): the config, the edge's box dump, pointer moves
# and drags, restarting the edge, the buttons it drew.
CFG = "/home/tester/.config/infynitae/config"
BOX = "/tmp/edge-boxes"
BACKUP = "/home/tester/.config/infynitae/panels.backup"
def line(name="CONTROL"):
    for l in open(CFG):
        if l.startswith(f"PANEL_{name}="): return l.strip()
    return ""
def boxes():
    wait(0.3)
    d = {"modules": {}}
    for l in open(BOX):
        w = l.split()
        if w[0] == "panel": d["panel"] = list(map(float, w[2:6])); d["state"] = w[6]
        elif w[0] == "module": d["modules"][w[1]] = list(map(float, w[2:6]))
        elif w[0] == "bar": d["bar"] = list(map(float, w[1:5]))
    return d
def at(x, y): move(-3000, -3000, 30); move(int(x), int(y), 30); wait(0.3)
def click_at(x, y, b="left"): at(x, y); click(b); wait(0.6)
def drag(x, y, dx, dy):
    at(x, y); emit((1, BTN["left"], 1)); wait(0.15)
    move(int(dx / 2), int(dy / 2), 30); wait(0.2); move(int(dx / 2), int(dy / 2), 30); wait(0.3)
    emit((1, BTN["left"], 0)); wait(0.8)
def restart_edge():
    sh("pkill -x infynitae-edge; true"); wait(1.0); run_bg(f"INFYNITAE_EDGE_BOXES={BOX} infynitae-edge"); wait(2.0)
# where the edge drew each button (its box dump: barbutton NAME x y w h, cardbutton MODULE NAME x y w h)
def buttons():
    wait(0.3)
    out = {}
    for l in open(BOX):
        w = l.split()
        if w and w[0] == "barbutton": out[("bar", w[1].lower())] = list(map(float, w[2:6]))
        elif w and w[0] == "cardbutton": out[(w[1], w[2].lower())] = list(map(float, w[3:7]))
    return out
def middle(r): return r[0] + r[2] / 2, r[1] + r[3] / 2
def bar_button(name): return middle(buttons()[("bar", name)])
def card_button(module, name): return middle(buttons()[(module, name)])
def hover_card(r): at(r[0] + 30, r[1] + r[3] - 8)

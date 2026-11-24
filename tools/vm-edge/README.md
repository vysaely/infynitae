# Edge tests with real input in the test VM

Run inside the VM (as root, they drive a virtual mouse and keyboard through /dev/uinput), with the
VM's helper scripts in ~/g (feat.py, fakempris.py, and everyday.py copied from this folder), vmlib.py
(the helpers the scripts share) and the scripts that others start from (pflow.py, papps.py, pcards.py) in ~, and the edge installed as
~/.local/bin/infynitae-edge:

    sudo -E python3 everyday.py   how people use the desktop, many tests; a throwaway VM only (it
                                  clicks Log Out, Reboot and Power Off)
    sudo -E python3 pflow.py      editing the default control panel (10 checks)
    sudo -E python3 pcards.py     Replay in Quick settings (5 checks; saves a real clip)
    sudo -E python3 pmic.py       the microphone row and the password prompt (3 checks)
    sudo -E python3 pfeel.py      everyday use start to end, with timings and screenshots (20 checks)
    sudo -E python3 pfluid.py     More and Network's list in place, the keyboard's selection, nothing
                                  left after closing (18 checks; z-* shots compared on the host with z-base)
    sudo -E python3 papps.py      the panel by its command, Network, Wi-Fi and power mode by click,
                                  hover, quick cycles (14 checks)
    sudo -E python3 ptime.py      key to first frame and Escape to closed, in ms
    sudo -E python3 pask.py       `infynitae-edge ask` end to end: a choice by keys and by click, a
                                  text, a secret, a cancel; the asker gets the answer and ends (7)
    sudo -E python3 pkbd.py [EDGE] typing into a terminal clicked while a panel is open, by hover and
                                  by key; NOIN=1 skips the click inside the panel (4)
    sudo -E python3 pfs.py EDGE    fullscreen on and off 5 times; the panel opens by key after each; counts
                                  null-buffer unmaps and layer changes in the edge's Wayland log (1)

Each prints PASS or FAIL per check and a SUMMARY line. They read the edge's INFYNITAE_EDGE_BOXES
dump: `panel`, one `card OWNER/KIND` per card, `hit OWNER/KIND ID` per control (the ids are constants
in the card's file under live/edge-apps/src/cards/), the edit bar's and card buttons, the Add list.

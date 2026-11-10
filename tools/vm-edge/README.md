# Edge tests with real input in the test VM

Run inside the VM (as root, they drive a virtual mouse and keyboard through /dev/uinput), with the
VM's helper scripts in ~/g (feat.py, everyday.py) and the edge installed as ~/.local/bin/infynitae-edge:

    sudo -E python3 pflow.py      editing a panel (11 checks)
    sudo -E python3 pcards.py     the Network and Replay cards (8 checks; saves a real clip)
    sudo -E python3 pmic.py       the microphone row and the password prompt (3 checks)
    sudo -E python3 pfeel.py      everyday use start to end, with timings and screenshots
    sudo -E python3 pchild.py     the cards that open beside a panel (7 checks)
    sudo -E python3 pwidgets.py   the desktop's widgets (12 checks)

Each prints PASS or FAIL per check and a SUMMARY line. They read the edge's INFYNITAE_EDGE_BOXES
dump (the panel's, each module's and the edit bar's boxes after every drawing).

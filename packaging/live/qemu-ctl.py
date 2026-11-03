#!/usr/bin/env python3
"""Pilote la machine virtuelle de packaging/live/qemu-test.sh --headless via
son moniteur HMP (dist/live/qemu/mon.sock) : capture d'écran, clavier,
souris (tablette USB : coordonnées absolues 0..32767).

usage : qemu-ctl.py screendump <fichier.png>
        qemu-ctl.py sendkey <touche>          (ex. ret, esc, ctrl-alt-t, a)
        qemu-ctl.py type <texte>              (lettres, chiffres, espace, ponctuation simple)
        qemu-ctl.py click <x> <y> <largeur> <hauteur>   (pixels de la capture → tablette)
        qemu-ctl.py hmp <commande HMP brute>
        qemu-ctl.py serial <ligne> [<ligne>…]   (console série : chaque ligne envoyée,
                                                 sortie lue 2 s ; image faite avec
                                                 SECMET_KERNEL_EXTRA=console=ttyS0,…)
"""
import os
import socket
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SOCK = os.environ.get("SECMET_QEMU_MON", os.path.join(ROOT, "dist", "live", "qemu", "mon.sock"))

KEYMAP = {
    " ": "spc", "-": "minus", "=": "equal", ".": "dot", ",": "comma", "/": "slash",
    ":": "shift-semicolon", "_": "shift-minus", "@": "shift-2", "~": "shift-grave_accent",
    "\n": "ret",
}


def hmp(cmd: str, wait: float = 0.3) -> str:
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.settimeout(5)
        s.connect(SOCK)
        time.sleep(0.2)
        try:
            s.recv(4096)  # bannière
        except socket.timeout:
            pass
        s.sendall((cmd + "\n").encode())
        time.sleep(wait)
        out = b""
        s.settimeout(1)
        try:
            while True:
                chunk = s.recv(65536)
                if not chunk:
                    break
                out += chunk
        except socket.timeout:
            pass
        text = out.decode(errors="replace")
        # Le moniteur renvoie l'écho de la commande avec des séquences de
        # terminal : on ne garde que les lignes de réponse.
        import re
        text = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", text)
        # L'écho est tapé caractère par caractère : on ne garde que ce qui
        # suit la dernière occurrence complète de la commande.
        idx = text.rfind(cmd)
        if idx >= 0:
            text = text[idx + len(cmd):]
        lines = [l.strip() for l in text.replace("\r", "\n").split("\n")]
        return "\n".join(l for l in lines if l and not l.startswith("(qemu)"))


# Disposition du clavier de l'invité : `sendkey` envoie des codes de touches
# (positions US) ; en AZERTY (image SecMet) les lettres a/q, z/w, m et les
# chiffres sont ailleurs. SECMET_QEMU_LAYOUT=us pour un invité en QWERTY.
LAYOUT = os.environ.get("SECMET_QEMU_LAYOUT", "fr")
FR_LETTERS = {"a": "q", "q": "a", "z": "w", "w": "z", "m": "semicolon"}
FR_OTHERS = {
    " ": "spc", "\n": "ret", "-": "6", "_": "8", ".": "shift-comma", ",": "m",
    "/": "shift-dot", ":": "dot", "@": "altgr-0", "=": "equal",
}


QMP = os.environ.get("SECMET_QEMU_QMP", os.path.join(os.path.dirname(SOCK), "qmp.sock"))


def qmp(command: str, arguments: dict | None = None) -> dict:
    """Commande QMP (JSON) : utilisée pour la souris absolue (tablette)."""
    import json
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.settimeout(5)
        s.connect(QMP)
        f = s.makefile("rwb", buffering=0)
        f.readline()  # bannière
        f.write(b'{"execute":"qmp_capabilities"}\n')
        f.readline()
        msg = {"execute": command}
        if arguments:
            msg["arguments"] = arguments
        f.write((json.dumps(msg) + "\n").encode())
        return json.loads(f.readline() or b"{}")


SERIAL = os.environ.get("SECMET_QEMU_SERIAL", os.path.join(os.path.dirname(SOCK), "serial.sock"))


def serial(lines: list[str], wait: float = 2.0) -> str:
    """Envoie chaque ligne sur la console série de l'invité et rend tout ce
    qui revient (getty sur ttyS0 : « secmet » / mot de passe pour ouvrir
    un shell). Les séquences de terminal sont retirées."""
    import re
    out = b""
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.connect(SERIAL)
        s.settimeout(0.5)
        for line in lines:
            s.sendall((line + "\n").encode())
            deadline = time.monotonic() + wait
            while time.monotonic() < deadline:
                try:
                    chunk = s.recv(65536)
                    if not chunk:
                        break
                    out += chunk
                except socket.timeout:
                    pass
    text = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", out.decode(errors="replace"))
    return text.replace("\r", "")


def keyname(ch: str) -> str:
    if LAYOUT == "fr":
        if ch in FR_OTHERS:
            return FR_OTHERS[ch]
        if ch.isalpha():
            base = FR_LETTERS.get(ch.lower(), ch.lower())
            return ("shift-" + base) if ch.isupper() else base
        if ch.isdigit():
            return "shift-" + ch
        raise ValueError(f"caractère non pris en charge (fr) : {ch!r}")
    if ch in KEYMAP:
        return KEYMAP[ch]
    if ch.isalpha():
        return ("shift-" + ch.lower()) if ch.isupper() else ch
    if ch.isdigit():
        return ch
    raise ValueError(f"caractère non pris en charge : {ch!r}")


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    cmd, args = argv[1], argv[2:]
    if cmd == "screendump":
        print(hmp(f"screendump {args[0]} -f png", wait=1.0).strip())
    elif cmd == "sendkey":
        print(hmp(f"sendkey {args[0]}").strip())
    elif cmd == "type":
        for ch in " ".join(args):
            hmp(f"sendkey {keyname(ch)}", wait=0.05)
    elif cmd == "click":
        x, y, w, h = (int(a) for a in args[:4])
        ax, ay = x * 32767 // w, y * 32767 // h
        move = [{"type": "abs", "data": {"axis": "x", "value": ax}},
                {"type": "abs", "data": {"axis": "y", "value": ay}}]
        qmp("input-send-event", {"events": move})
        time.sleep(0.1)
        qmp("input-send-event", {"events": [{"type": "btn", "data": {"down": True, "button": "left"}}]})
        time.sleep(0.1)
        qmp("input-send-event", {"events": [{"type": "btn", "data": {"down": False, "button": "left"}}]})
    elif cmd == "hmp":
        print(hmp(" ".join(args)).strip())
    elif cmd == "serial":
        print(serial(args))
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

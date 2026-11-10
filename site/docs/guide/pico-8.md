# PICO-8

slot can run [PICO-8](https://www.lexaloffle.com/pico-8.php) carts and Splore, PICO-8's built-in browser for carts on the Lexaloffle BBS.

slot does not include PICO-8 itself. You need to buy it from Lexaloffle, which licenses it per person, and copy your own copy onto the card.

## Setting it up

1. Download the **Raspberry Pi** build from your Lexaloffle account, e.g. `pico-8_0.2.6b_raspi.zip`.
2. Copy the zip, as it is, into `BIOS/PICO-8/` on the card. You can also unzip it there yourself, as long as `pico8_dyn` and `pico8.dat` end up in `BIOS/PICO-8/`.
3. Put your carts in `Games/PICO8/`. Both `.p8` and `.p8.png` carts work.

Next time slot starts, a PICO-8 shelf appears with a **Splore** cart on it. slot unpacks the zip the first time you play something.

!!! info "No PICO-8, no play"

    Without your copy of PICO-8 in `BIOS/PICO-8/`, the shelf still lists your carts, but slot refuses to start them.

## Playing

Press <kbd>A</kbd> on a cart to play it, or on Splore to browse the BBS. The screen goes dark for a moment while slot steps aside and PICO-8 starts.

| Button | In PICO-8 |
|---|---|
| D-pad / stick | Arrows |
| <kbd>B</kbd> | O |
| <kbd>A</kbd> | X |
| <kbd>START</kbd> | PICO-8's pause menu |
| <kbd>MENU</kbd> | Quit PICO-8 and go back to slot |
| <kbd>POWER</kbd> | Screen off and pause, press again to carry on |
| Closing the lid | Screen off and pause until you open it |

When PICO-8 quits, slot comes back on the same cart.

The things slot does for its own games, like save states, rewind, bezels and the in-game menu, are not there in PICO-8. Games that save, save on their own.

## Splore

Splore needs the internet. It joins the Wi-Fi networks you saved in slot's Network menu and goes back offline when you leave.

Carts and favourites from Splore stay in PICO-8's own lists, not on the shelf.

## Labels

slot uses the label drawn into each cart. A cart that has none gets a printed title. To use your own, put a PNG with the same name in `Labels/PICO8/`.

## Where things are

| Folder | What's in it |
|---|---|
| `BIOS/PICO-8/` | Your copy of PICO-8 |
| `Games/PICO8/` | Your carts |
| `Labels/PICO8/` | Labels for the shelf |
| `Saves/PICO8/` | PICO-8's home: game saves (`cdata/`), `config.txt`, Splore's downloads and favourites |
| `Screenshots/PICO8/` | Screenshots and GIFs taken in PICO-8 |
| `Logs/pico8.log` | What happened the last time PICO-8 ran |

To change how PICO-8 draws or sounds, edit `Saves/PICO8/config.txt`. PICO-8 writes it the first time it runs.

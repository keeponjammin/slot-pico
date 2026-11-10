# PICO-8

slot plays [PICO-8](https://www.lexaloffle.com/pico-8.php) carts on [fake-08](https://github.com/jtothebell/fake-08), a free PICO-8 core that comes with slot. If you own PICO-8, slot can also run carts on your own copy, and open Splore, PICO-8's built-in browser for carts on the Lexaloffle BBS.

## Setting it up

Put your carts in `Games/PICO8/`. Both `.p8` and `.p8.png` carts work. Next time slot starts, a PICO-8 shelf appears.

## Playing

Press <kbd>A</kbd> on a cart to play it. It goes into the slot and plays like any other game, with slot's in-game menu, save states, screenshots and the quick menu. Rewind, bezels and overlays are not there for PICO-8. Its own Shader and Scale are under Settings → Screen → PICO-8, apart from the Game Boy's.

| Button | In PICO-8 |
|---|---|
| D-pad / stick | Arrows |
| <kbd>B</kbd> | O |
| <kbd>A</kbd> | X |
| <kbd>START</kbd> | The cart's pause menu |

Games that save, like high scores, keep their saves in `Saves/PICO8/cdata/`, the same place and the same files your own PICO-8 uses. A game saved on one carries on on the other.

## Your own PICO-8

slot does not include PICO-8 itself. You can buy it from Lexaloffle, which licenses it per person, and copy your own copy onto the card:

1. Download the **Raspberry Pi** build from your Lexaloffle account, e.g. `pico-8_0.2.6b_raspi.zip`.
2. Copy the zip, as it is, into `BIOS/PICO-8/` on the card. You can also unzip it there yourself, as long as `pico8_dyn` and `pico8.dat` end up in `BIOS/PICO-8/`.

Next time slot starts, a **Splore** cart appears on the PICO-8 shelf. slot unpacks the zip the first time you play something on it.

To play a cart on your own PICO-8 instead of fake-08, press <kbd>START</kbd> on it and pick PICO-8 on the board, as you would pick a core for a Game Boy game. Splore always opens in your own PICO-8.

!!! info "No PICO-8, no Splore"

    Without your copy of PICO-8 in `BIOS/PICO-8/`, there is no Splore cart, and slot refuses to start a cart set to PICO-8.

When a cart runs on your own PICO-8, the screen goes dark for a moment while slot steps aside and PICO-8 starts.

| Button | In PICO-8 |
|---|---|
| D-pad / stick | Arrows |
| <kbd>B</kbd> | O |
| <kbd>A</kbd> | X |
| <kbd>START</kbd> | PICO-8's pause menu |
| <kbd>MENU</kbd> | Pause under slot's in-game menu |
| Hold <kbd>MENU</kbd> | Quit PICO-8 and go back to slot |
| <kbd>POWER</kbd> | Screen off and pause, press again to carry on |
| Closing the lid | Screen off and pause until you open it |

When PICO-8 quits, slot comes back on the same cart.

The in-game menu has two rows for PICO-8: Resume, or Exit Game to go back to slot. <kbd>B</kbd> or another tap of <kbd>MENU</kbd> also resumes.

The other things slot does for its own games, like save states, rewind and bezels, are not there in your own PICO-8. Games that save, save on their own.

## Splore

Splore needs the internet. It joins the Wi-Fi networks you saved in slot's Network menu and goes back offline when you leave.

Carts and favourites from Splore stay in PICO-8's own lists, not on the shelf.

## Labels

slot uses the label drawn into each cart. A cart that has none gets a printed title. To use your own, put a PNG with the same name in `Labels/PICO8/`.

## Where things are

| Folder | What's in it |
|---|---|
| `BIOS/PICO-8/` | Your own copy of PICO-8, if you have one |
| `Games/PICO8/` | Your carts |
| `Labels/PICO8/` | Labels for the shelf |
| `Saves/PICO8/` | Game saves (`cdata/`) for both fake-08 and your own PICO-8, and your PICO-8's `config.txt`, Splore's downloads and favourites |
| `Screenshots/PICO8/` | Screenshots and GIFs taken in your own PICO-8 |
| `Logs/pico8.log` | What happened the last time PICO-8 ran |

To change how your own PICO-8 draws or sounds, edit `Saves/PICO8/config.txt`. PICO-8 writes it the first time it runs.

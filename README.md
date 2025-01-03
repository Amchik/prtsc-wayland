# `prtsc-wayland` (WIP)

> 📸 Screenshot utility for wayland

 ## Why?

 `grim` + `slurp` is good way but it doesn't "freeze" screen to screenshot. For example,
 using slurp you can't screen tooltips and some other things.

 This app make screenshot of full display then allows to select region to capture in frozen screen.

 **TL;DR:** this app allows to screenshot hover-only elements.

 ## Building and usage

 To build, run `cargo build --release`. Executable will located in `target/release/prtsc-wayland`.

 Usage:
 ```console
 $ prtsc-wayland -h
 Usage: prtsc-wayland [OPTIONS]

Options:
  -o, --output <OUTPUT>  File to save screenshot [default: image.png]
  -f, --fullscreen       Do not use region selector
  -h, --help             Print help
 ```

 I don't know what formats are supported, see [docs.rs/image](https://docs.rs/image) if you really
 interested. Fullscreen mode (`-f`) is just default grim behavior, I added it just for fun.

 To exit selection press <kbd>Esc</kbd>. Press it again to exit overlay.

## Bugs

- I didn't add selection cleaning, so resizing is ugly.
- Pointers...


# `prtsc-wayland`

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
Wayland screenshot utility

Usage: prtsc-wayland [OPTIONS]

Options:
  -o, --output <OUTPUT>
          File to save screenshot (use '-' to output to stdout) [default: image.png]
  -f, --fullscreen
          Do not use region selector
  -s, --selection-only
          Only make region selection and print it
  -F, --selection-format <SELECTION_FORMAT>
          If --selection-only, format of selection output [default: "%x,%y %wx%h%n"]
  -h, --help
          Print help
  -V, --version
          Print version

Formatting:
  %x %X The x-coordinate of the selection
  %y %Y The y-coordinate of the selection
  %w %W The width of the selection
  %h %H The height of the selection
  %o    The name of output
  %n    Newline char ('\n')
```

I don't know what formats are supported, see [docs.rs/image](https://docs.rs/image) if you really
interested. Fullscreen mode (`-f`) is just default grim behavior (making screenshot without drawing
something on screen), I added it just for fun.

To exit selection press <kbd>Esc</kbd>. Press it again to exit overlay.

To move region during selection hold <kbd>Space</kbd>.

## Thanks

- [grim](https://sr.ht/~emersion/grim/) and [slurp](https://github.com/emersion/slurp)
- [this example from `smithay-client-toolkit`](https://github.com/Smithay/client-toolkit/blob/master/examples/simple_layer.rs)


# Splash screens

The picture shown while Jank! starts. The program picks one by today's date, so holidays get their own art.

## How a start looks
1. The bar fills to about 40% (the intro).
2. It then follows the real loading of the editor.
3. The splash stays for at least two seconds and fades out in 0.18 s.

Timings are constants in `app/src/splash.rs` (`MIN_SECS`) and `app/src/startup.rs`.

## Where things live
| What | Where |
|---|---|
| Pictures | `app/assets/splash/` (JPEG, 1280 x 1024) |
| Which picture on which date | `core/src/splash.rs`, table `RULES` |
| Picture bytes compiled into the program | `app/src/splash.rs`, function `bytes()` |

## Adding a new splash picture
1. Make the picture 1280 x 1024 and save it as JPEG in `app/assets/splash/`. Name it `<id>_<MMDD>-<MMDD>.jpg` for a holiday (`christmas_1224-1225.jpg`) or `<id>_<variant>.jpg` for the default set (`default_c.jpg`).
2. Add the file name to the right `Rule` in `core/src/splash.rs`. For a new holiday, add a `Season` variant, a `Rule` with its date ranges, and the variant to `Season::ALL`. A range that starts after it ends wraps over the new year.
3. Add one line to `bytes()` in `app/src/splash.rs` with `include_bytes!`.
4. Run `cargo test` in `core` and in `app`: the app test checks that every file named in the table is embedded, matches the file on disk and decodes at 1280 x 1024; the core tests check the date rules.

## Previewing
```
jank --splash christmas        # force a season (id or number 1-5)
jank --date 12-25              # pretend it is that day
jank --no-splash               # skip it
```
Environment variables `JANK_SPLASH`, `JANK_DATE` and `JANK_NO_SPLASH` do the same.

## Notes
- Several files in one `Rule` are variants; one is picked per launch.
- The current pictures have the build number painted into the art. It is not updated automatically.

# GNU Unifont 18.0.01

`unifont-18.0.01.hex.gz` (plane 0) and `unifont_upper-18.0.01.hex.gz` (planes 1 and up), unmodified, from
<https://unifoundry.com/pub/unifont/unifont-18.0.01/font-builds/>.

The glyphs are dual-licensed: SIL Open Font License 1.1 (`OFL-1.1.txt`), or GNU GPL version 2 or later with the font embedding exception (`COPYING`). Trinux uses them under the OFL.

`mkdisk` (`crates/trinux/src/unifont.rs`) packs planes 0 and 1 into `/usr/share/unifont` on the disk image; see docs/KERNEL.md §21.

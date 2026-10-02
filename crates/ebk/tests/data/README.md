# Test data

`alloc.br`: a brotli stream for the allocation-failure test in `src/format.rs`.

`*.jpg`: pictures made by `make_jpegs.py` (generated, not taken from anywhere).

`*.lep`: Lepton files that a reader of storage mode 4 must accept (specification, section 7), as written by the
reference writer (`lepton_jpeg` 0.5.8, one partition), and the JPEG file each must decode to. The test
`stored_lepton_files_decode_to_the_same_jpegs` reads them through the reader. `no-eoi.lep` is written by the fork
in `third_party/lepton_jpeg` (change 19 there): the published encoder writes a file for `no-eoi.jpg` that decodes
to other bytes; the published decoder reads this one.

| Lepton file | bytes | decodes to | bytes | CRC-32 | SHA-256 |
|---|---|---|---|---|---|
| `gray.lep` | 11122 | `gray.jpg` | 14595 | 530e341c | fcb2c75f242dd870544a4664659390ba405c8e1fb1a16262f5cdb749c52ce7fe |
| `color.lep` | 26794 | `color.jpg` | 31793 | 4be1da24 | 82c8a0e4398529c485e80416c341b75f8b84028ba70a513191ce195b8543ee0c |
| `progressive.lep` | 26754 | `progressive.jpg` | 28977 | 0d92552b | ac9127824496e721fd0266c1ba1b0571b2a8e2fefb59eff865d7379952389417 |
| `trailing.lep` | 26823 | `trailing.jpg` | 31825 | a292e16a | ef053915fc951ea9ab480f770098b751994831304293e4102342152c9c98d1e1 |
| `no-eoi.lep` | 26795 | `no-eoi.jpg` | 31791 | be3e935d | 9a51bc865e9f5ff4af099cc3bfd838053cfe94657cac1e0772738ef317f9689f |

# Cheat-checker
Detects similarities between sets of files, intended to detect academic dishonesty.
## Installation
1. Install rust, either directly through your system's package manager, or by installing `rustup` and running `rustup install stable`.
2. Run `cargo install cheat_checker`.
3. Done! Run `cheat_checker --help` for usage instructions.
## Speed
Yeah, it's quite slow. The reason for making this was mostly the UX, not the speed, but I did try to optimize it. I did some benchmarks, and it turns out the `python-Levenshtein` library for python is about 16 times faster than `eddie` (which is what this program uses) and `strsim`. It's written in C or C++, and pretty arcane C/C++ at that. I did what I could and added some multithreading, but on my 4-core laptop, it's still about 8 times slower than using `python-Levenshtein` single-threaded. 

Heavily inspired by [copy_checker](https://gitlab.com/classroomcode/copy_checker).
Licensed under the GNU General Public License V3.0.

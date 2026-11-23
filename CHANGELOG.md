# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [v0.6.0](https://github.com/p-chan/mxm/compare/v0.5.0...v0.6.0)

### Added

- add `mxm service update` to update the daemon after an update ([#25](https://github.com/p-chan/mxm/pull/25))

### Other

- open release PRs with a GitHub App token ([#24](https://github.com/p-chan/mxm/pull/24))

## [v0.5.0](https://github.com/p-chan/mxm/compare/v0.4.1...v0.5.0)

### Added

- notarize macOS release binaries ([#22](https://github.com/p-chan/mxm/pull/22))

### Other

- describe mxm as a CLI and daemon with Options+-style scrolling ([#21](https://github.com/p-chan/mxm/pull/21))

## [v0.4.1](https://github.com/p-chan/mxm/compare/v0.4.0...v0.4.1)

### Fixed

- notice the Accessibility permission granted while the daemon runs ([#19](https://github.com/p-chan/mxm/pull/19))
- don't panic when `mxm service` output is cut off ([#18](https://github.com/p-chan/mxm/pull/18))

## [v0.4.0](https://github.com/p-chan/mxm/compare/v0.3.0...v0.4.0)

### Added

- scroll the wheels in software like Logi Options+ ([#16](https://github.com/p-chan/mxm/pull/16))

## [v0.3.0](https://github.com/p-chan/mxm/compare/v0.2.0...v0.3.0)

### Added

- run the service from a copy of mxm at a fixed path ([#14](https://github.com/p-chan/mxm/pull/14))

## [v0.2.0](https://github.com/p-chan/mxm/compare/v0.1.0...v0.2.0)

### Added

- sign macOS release binaries with Developer ID ([#11](https://github.com/p-chan/mxm/pull/11))

### Other

- bump the minor version for features in 0.x ([#13](https://github.com/p-chan/mxm/pull/13))
- use v-prefixed changelog headings without dates ([#9](https://github.com/p-chan/mxm/pull/9))
- open release PRs only for feat and fix commits ([#10](https://github.com/p-chan/mxm/pull/10))

## [v0.1.0](https://github.com/p-chan/mxm/releases/tag/v0.1.0)

### Added

- add service command to run the daemon at login ([#8](https://github.com/p-chan/mxm/pull/8))
- add config file and daemon mode ([#5](https://github.com/p-chan/mxm/pull/5))
- support MX Master 3S ([#2](https://github.com/p-chan/mxm/pull/2))
- add HID++ 2.0 CLI for MX Master mice

### Other

- release macOS binaries with release-plz and dist ([#6](https://github.com/p-chan/mxm/pull/6))
- add fmt, clippy and test workflow ([#4](https://github.com/p-chan/mxm/pull/4))
- add MIT license ([#3](https://github.com/p-chan/mxm/pull/3))
- rename project to mxm ([#1](https://github.com/p-chan/mxm/pull/1))

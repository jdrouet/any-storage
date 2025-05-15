# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1](https://github.com/jdrouet/any-storage/compare/v0.3.0...v0.3.1) - 2025-05-15

### Fixed

- check pcloud config deserializer

### Other

- add repository in cargo.toml

## [0.3.0](https://github.com/jdrouet/any-storage/compare/v0.2.2...v0.3.0) - 2025-05-15

### Added

- add name and path function to file and directory
- create noop store
- support deletion ([#7](https://github.com/jdrouet/any-storage/pull/7))

### Other

- mark all AnyStore variants non exhaustive
- remove delete support from todo
- release cargo doc as github page

## [0.2.2](https://github.com/jdrouet/any-storage/compare/v0.2.1...v0.2.2) - 2025-05-11

### Added

- add builder for AnyStoreConfig

## [0.2.1](https://github.com/jdrouet/any-storage/compare/v0.2.0...v0.2.1) - 2025-05-11

### Other

- add readme in cargo.toml

## [0.2.0](https://github.com/jdrouet/any-storage/compare/v0.1.0...v0.2.0) - 2025-05-11

### Added

- add root path for pcloud store
- add config for clients
- create AnyStore

### Fixed

- make sure the given path is absolute in pcloud
- update credentials in pcloud tests

## [0.1.0](https://github.com/jdrouet/any-storage/releases/tag/v0.1.0) - 2025-05-08

### Added

- create file writer implementation ([#2](https://github.com/jdrouet/any-storage/pull/2))
- implement pcloud store
- add root function on store
- url Url in HttpStore
- add metadata on files
- add http store
- implement a http store
- create simple reader for local files
- init project

### Fixed

- readme code

### Other

- install release-plz
- add table with feature support
- update readme
- *(deps)* Bump tokio from 1.44.2 to 1.45.0 ([#1](https://github.com/jdrouet/any-storage/pull/1))
- format comments
- add readme
- add comments on pcloud store
- add comments on http store
- add comments on local store
- add comments
- configuration actions
- root with default PathBuf
- *(http)* embed path and store instead of full url
- *(http)* wrap everything in inner store
- format code
- check with can scan http store

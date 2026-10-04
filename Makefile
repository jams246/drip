SHELL := cmd.exe
.SHELLFLAGS := /D /C

.PHONY: build build\:production build\:docker

build:
	pwsh.exe -NoProfile -File scripts/build.ps1

build\:production:
	pwsh.exe -NoProfile -File scripts/build.ps1 -Production

build\:docker:
	pwsh.exe -NoProfile -File scripts/build-docker.ps1

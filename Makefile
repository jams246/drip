SHELL := cmd.exe
.SHELLFLAGS := /D /C

.PHONY: build build\:production

build:
	pwsh.exe -NoProfile -File scripts/build.ps1

build\:production:
	pwsh.exe -NoProfile -File scripts/build.ps1 -Production

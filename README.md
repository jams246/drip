# Drip

**D**irected **R**emote **I**ncremental **P**ush

## About The Project

Drip was built to be a simple and easy to set up file syncing app and will not come with many of the features you find in similar apps.
Things like  _version history_, _bi-directional sync_, and _file sharing links_ etc.. are not present and will never be present.

Drip simply mirrors selected files and folders from your Windows computer to your server. Deletes are also propagated to the mirror.

That's it.

## Features

* One-way sync from your PC (Windows only) to your own self-hosted server.
* File and folder selection (including recursive folder scanning)
* Incremental uploads that only transfers what has changed.
* FastCDC content chucking and BLAKE3 hashing for (hopefully) efficient transfers and integrity checks
* Background operation with Windows system tray controls.

## How it works

1. Run the Drip server using the docker image.
2. Generate a registration token.
    1. The token only lasts for 15 minutes.
3. Start the drip.exe app and select which files and folders to watch.
4. Drip handles the rest.

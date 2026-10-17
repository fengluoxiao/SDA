# Tracking Startup

When the saved tracking preference is enabled, the native module starts Core
Motion while foreground even before a playback engine exists. The callback
retains at most twelve real samples. Playback reuses the subscription and feeds
only samples from the last half second to the shared helper calibration. No
synthetic calibration samples or JS pose processing are introduced.

An identity pose selects the tracked short render reserve before any source PCM
is fed. The yaw-only startup command must not clear that pose while tracking is
enabled. Apple system spatial playback explicitly stops SDA tracking.

The Windows helper filter and KU100 pre-convolution direction selection are
unchanged. Physical iPhone tests must check saved-on cold launch, immediate play,
play after waiting one second, track changes, background return and screen
capture recovery. Core Motion startup/permission delays cannot be eliminated
when the provider has no fresh samples; unstable calibration still waits.

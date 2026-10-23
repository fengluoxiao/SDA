package com.sda.sine

import android.app.Activity
import android.os.Bundle
import android.widget.TextView

class MainActivity : Activity() {
    init { System.loadLibrary("sda_aaudio_sine") }

    private external fun startSine(): Int

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val rc = startSine()
        val tv = TextView(this)
        tv.textSize = 20f
        tv.setPadding(48, 96, 48, 48)
        tv.text = if (rc == 0) "sine playing (440 Hz)\nlogcat tag: SdaSine" else "startSine failed: $rc"
        setContentView(tv)
    }
}

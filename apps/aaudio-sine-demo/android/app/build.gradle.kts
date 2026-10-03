plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.sda.sine"
    compileSdk = 34
    defaultConfig {
        applicationId = "com.sda.sine"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "0.1"
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    sourceSets { getByName("main") { jniLibs.srcDir("../jniLibs") } }
    // MuMu (Android 12 image) segfaults in the audio callback when the .so is
    // mapped straight out of the APK (AGP 8 default useLegacyPackaging=false):
    // first process runs, later processes take SEGV_ACCERR executing the
    // callback. Extract libs to real files to sidestep the emulator W^X bug.
    packaging { jniLibs { useLegacyPackaging = true } }
}

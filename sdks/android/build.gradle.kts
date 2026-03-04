plugins {
    id("com.android.library")
    kotlin("android")
}

android {
    namespace = "com.experimentation.sdk"
    compileSdk = 34

    defaultConfig {
        minSdk = 26
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.0")
    // ConnectRPC Kotlin will be added when Agent-1 implements transport
    // implementation("com.connectrpc:connect-kotlin:0.6.0")
    // implementation("com.connectrpc:connect-kotlin-okhttp:0.6.0")

    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.8.0")
    testImplementation("com.google.truth:truth:1.4.2")
}

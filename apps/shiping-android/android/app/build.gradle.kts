import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "com.dripai.shiping"
    compileSdk = 36

    defaultConfig {
        val releaseVersion = providers.gradleProperty("shipingVersion")
            .orElse("0.1.31")
            .get()
        val versionParts = releaseVersion.split('.').map(String::toInt)
        require(versionParts.size == 3) {
            "shipingVersion must use major.minor.patch format"
        }

        applicationId = "com.dripai.shiping"
        minSdk = 26
        targetSdk = 36
        versionCode = versionParts[0] * 1_000_000 +
            versionParts[1] * 1_000 +
            versionParts[2]
        versionName = releaseVersion

        ndk {
            abiFilters += "arm64-v8a"
        }
    }

    sourceSets["main"].jniLibs.srcDir("../native-libs")

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        jniLibs {
            useLegacyPackaging = false
        }
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2026.06.00")

    implementation(composeBom)
    androidTestImplementation(composeBom)

    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.core:core-ktx:1.17.0")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.10.0")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")

    debugImplementation("androidx.compose.ui:ui-tooling")

    testImplementation("junit:junit:4.13.2")
}

val verifyRustLibrary by tasks.registering {
    val library = file("../native-libs/arm64-v8a/libshiping_android.so")
    doLast {
        check(library.isFile) {
            "Missing Rust Android library: ${library.absolutePath}"
        }
    }
}

tasks.named("preBuild") {
    dependsOn(verifyRustLibrary)
}

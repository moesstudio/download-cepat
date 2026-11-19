import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

val keystoreProps = Properties().apply {
    val kpFile = rootProject.file("keystore.properties")
    if (kpFile.exists()) {
        kpFile.inputStream().use { load(it) }
    }
}

val releaseStorePath = System.getenv("ANDROID_KEYSTORE_PATH")
    ?: keystoreProps.getProperty("storeFile")
val releaseStorePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
    ?: keystoreProps.getProperty("storePassword")
val releaseKeyAlias = System.getenv("ANDROID_KEY_ALIAS")
    ?: keystoreProps.getProperty("keyAlias")
val releaseKeyPassword = System.getenv("ANDROID_KEY_PASSWORD")
    ?: keystoreProps.getProperty("keyPassword")
val hasReleaseSigning = !releaseStorePath.isNullOrBlank()
    && file(releaseStorePath).exists()
    && !releaseStorePassword.isNullOrBlank()
    && !releaseKeyAlias.isNullOrBlank()
    && !releaseKeyPassword.isNullOrBlank()

android {
    compileSdk = 36
    namespace = "com.moesstudio.downloadcepat"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "true"
        applicationId = "com.moesstudio.downloadcepat"
        minSdk = 26
        targetSdk = 35
        val rawVersionName = tauriProperties.getProperty("tauri.android.versionName", "1.0.5")
        val semverParts = rawVersionName.substringBefore("-").split(".")
        val computedSemverCode = if (semverParts.size >= 3) {
            val maj = semverParts[0].toIntOrNull() ?: 1
            val min = semverParts[1].toIntOrNull() ?: 0
            val pat = semverParts[2].toIntOrNull() ?: 0
            maj * 10000 + min * 100 + pat
        } else {
            1
        }
        val propVersionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toIntOrNull() ?: 1
        versionCode = maxOf(propVersionCode, computedSemverCode)
        versionName = rawVersionName
    }
    signingConfigs {
        if (hasReleaseSigning) {
            create("release") {
                storeFile = file(releaseStorePath!!)
                storePassword = releaseStorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
                enableV1Signing = true
                enableV2Signing = true
                enableV3Signing = true
            }
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            if (hasReleaseSigning) {
                signingConfig = signingConfigs.getByName("release")
            }
            optimization {
               enable = true
            }
            proguardFiles(
                *fileTree(".") {
                  include("**/*.pro")
                  exclude("build/**")
                }.files.toTypedArray()
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    buildFeatures {
        buildConfig = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = file("tauri.build.gradle.kts"))

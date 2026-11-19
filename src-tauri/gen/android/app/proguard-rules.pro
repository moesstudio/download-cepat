# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# Keep Tauri Android plugins, commands, InvokeArgs, and DownloadForegroundService across R8 optimization
-keep @app.tauri.annotation.TauriPlugin class * { *; }
-keep @app.tauri.annotation.InvokeArg class * { *; }
-keepclassmembers class * {
    @app.tauri.annotation.Command <methods>;
    @app.tauri.annotation.ActivityCallback <methods>;
    @app.tauri.annotation.PermissionCallback <methods>;
}
-keep class com.moesstudio.downloadcepat.DownloadBridgePlugin { *; }
-keep class com.moesstudio.downloadcepat.DownloadBridgePlugin$* { *; }
-keep class com.moesstudio.downloadcepat.DownloadForegroundService { *; }
-keep class com.moesstudio.downloadcepat.DownloadForegroundService$* { *; }

# Keep youtubedl-android, ffmpeg, Jackson, and Apache Commons reflection across R8 optimization
-keep class com.yausername.youtubedl_android.** { *; }
-keep class com.yausername.ffmpeg.** { *; }
-keep class com.yausername.youtubedl_common.** { *; }
-keep class com.fasterxml.jackson.** { *; }
-keep class org.apache.commons.** { *; }
-dontwarn java.beans.ConstructorProperties
-dontwarn java.beans.Transient
-dontwarn org.apache.commons.**
-dontwarn org.tukaani.**
-keepattributes *Annotation*,Signature,InnerClasses,EnclosingMethod
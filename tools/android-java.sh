#!/bin/sh
# Adds the small Java part (android/java: file picker, sharing, key store, recording service) to an APK built by
# cargo-apk, then signs it again. Run after every `cargo apk build`:
#   sh tools/android-java.sh target/release/apk/LeafNotes.apk
set -e
APK=${1:-target/release/apk/LeafNotes.apk}
SDK=${ANDROID_HOME:?set ANDROID_HOME}
BT=$(ls -d "$SDK"/build-tools/* | sort -V | tail -1)
JAR="$SDK/platforms/android-34/android.jar"
OUT=target/android-java
rm -rf "$OUT" && mkdir -p "$OUT/classes"
javac -nowarn -Xlint:-options -source 8 -target 8 -bootclasspath "$JAR" -d "$OUT/classes" $(find android/java -name '*.java')
"$BT/d8" --release --min-api 26 --lib "$JAR" --output "$OUT" $(find "$OUT/classes" -name '*.class')
# cargo-apk can't declare a service or a provider: add them to its manifest and package it again
# (the recording service, which keeps the microphone on with the screen off, and FileShare for Android 8/9)
MAN=$(dirname "$APK")/AndroidManifest.xml
sed -e 's|<uses-permission android:name="android.permission.RECORD_AUDIO"/>|&<uses-permission android:name="android.permission.FOREGROUND_SERVICE"/><uses-permission android:name="android.permission.FOREGROUND_SERVICE_MICROPHONE"/>|' \
    -e 's|</application>|<service android:name="org.leafnotes.app.RecordService" android:foregroundServiceType="microphone" android:exported="false"/><provider android:name="org.leafnotes.app.FileShare" android:authorities="org.leafnotes.app.files" android:exported="false" android:grantUriPermissions="true"/></application>|' \
    "$MAN" > "$OUT/AndroidManifest.xml"
grep -q RecordService "$OUT/AndroidManifest.xml" || { echo "couldn't add the recording service to the manifest"; exit 1; }
"$BT/aapt" package -f -F "$OUT/app.apk" -M "$OUT/AndroidManifest.xml" -S android/res -I "$JAR"
mkdir -p "$OUT/libs" && unzip -q -o "$APK" 'lib/*' -d "$OUT/libs"
(cd "$OUT/libs" && zip -q -r ../app.apk lib)
(cd "$OUT" && zip -q app.apk classes.dex)
"$BT/zipalign" -f -p 4 "$OUT/app.apk" "$OUT/aligned.apk"
"$BT/apksigner" sign --ks android/dev.keystore --ks-pass pass:android --out "$APK" "$OUT/aligned.apk"
echo "Java part added: $APK"

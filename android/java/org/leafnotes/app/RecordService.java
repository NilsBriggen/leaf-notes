package org.leafnotes.app;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;

/**
 * Keeps a lecture recording going while the screen is off or another app is in front: Android only
 * lets an app hear the microphone in the background while it shows this "recording" notification.
 * The recording itself is done by the Rust side; this only holds the app in the foreground.
 */
public class RecordService extends Service {
    private static final String CHANNEL = "recording";

    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        NotificationManager nm = (NotificationManager) getSystemService(NOTIFICATION_SERVICE);
        if (nm != null) nm.createNotificationChannel(new NotificationChannel(CHANNEL, "Recording", NotificationManager.IMPORTANCE_LOW));
        Intent open = new Intent(this, LeafActivity.class).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP);
        PendingIntent tap = PendingIntent.getActivity(this, 0, open, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        Notification n = new Notification.Builder(this, CHANNEL)
            .setContentTitle("Leaf Notes is recording")
            .setContentText("Tap to go back to your notes")
            .setSmallIcon(android.R.drawable.ic_btn_speak_now)
            .setContentIntent(tap)
            .setOngoing(true)
            .build();
        if (Build.VERSION.SDK_INT >= 29) startForeground(1, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE);
        else startForeground(1, n);
        return START_NOT_STICKY;
    }

    @Override public IBinder onBind(Intent intent) { return null; }
}

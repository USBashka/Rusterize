package dev.rusterize;
import android.content.Context;
import android.media.*;
import android.os.*;

final class NativeAudio {
    private final AudioManager manager;
    private final AudioFocusRequest request;
    NativeAudio(final RusterizeView view){
        manager=(AudioManager)view.getContext().getSystemService(Context.AUDIO_SERVICE);
        request=new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_GAME).setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
            .setOnAudioFocusChangeListener(new AudioManager.OnAudioFocusChangeListener(){
                @Override public void onAudioFocusChange(int focus){if(focus!=AudioManager.AUDIOFOCUS_GAIN)view.audioInterrupted();}
            },new Handler(Looper.getMainLooper())).build();
    }
    void acquire(){if(manager.requestAudioFocus(request)!=AudioManager.AUDIOFOCUS_REQUEST_GRANTED)throw new IllegalStateException("Audio focus was denied");}
    void release(){manager.abandonAudioFocusRequest(request);}
}

// Opt-in measurement executable. No timers, recording, or extra ownership in normal builds.
#include "open_daw/service.hpp"
#include "open_daw/device.hpp"
#include <algorithm>
#include <chrono>
#include <cmath>
#include <iostream>
#include <thread>
#include <iomanip>
#if defined(__APPLE__)
#include <mach/mach.h>
#include <CoreAudio/AudioHardware.h>
#endif
using Clock=std::chrono::steady_clock;
namespace {
#if defined(__APPLE__)
std::atomic<uint64_t> overloads{0};
OSStatus overloadListener(AudioObjectID,UInt32,const AudioObjectPropertyAddress*,void*){overloads.fetch_add(1,std::memory_order_relaxed);return noErr;}
struct OverloadMonitor {
    AudioDeviceID device=kAudioObjectUnknown;bool registered=false;
    AudioObjectPropertyAddress address{kAudioDeviceProcessorOverload,kAudioObjectPropertyScopeGlobal,kAudioObjectPropertyElementMain};
    OverloadMonitor(){AudioObjectPropertyAddress source{kAudioHardwarePropertyDefaultOutputDevice,kAudioObjectPropertyScopeGlobal,kAudioObjectPropertyElementMain};UInt32 size=sizeof(device);
        if(AudioObjectGetPropertyData(kAudioObjectSystemObject,&source,0,nullptr,&size,&device)==noErr)registered=AudioObjectAddPropertyListener(device,&address,overloadListener,nullptr)==noErr;}
    ~OverloadMonitor(){if(registered)AudioObjectRemovePropertyListener(device,&address,overloadListener,nullptr);}
};
#endif
uint64_t rss(){
#if defined(__APPLE__)
    mach_task_basic_info_data_t info{};mach_msg_type_number_t size=MACH_TASK_BASIC_INFO_COUNT;
    if(task_info(mach_task_self(),MACH_TASK_BASIC_INFO,reinterpret_cast<task_info_t>(&info),&size)==KERN_SUCCESS)return info.resident_size;
#endif
    return 0;
}
daw::SessionSpec mixed(uint32_t tracks){
    daw::SessionSpec s;s.tracks.resize(tracks);s.lengthFrames=48000*8;uint64_t id=1;
    auto sample=std::make_shared<daw::SampleData>();sample->sampleRate=48000;sample->stereo.resize(48000*10*2);
    for(size_t i=0;i<sample->frames();++i){const float value=.06f*static_cast<float>(std::sin(i*6.283185307179586*173/48000));sample->stereo[2*i]=value;sample->stereo[2*i+1]=value;}
    for(uint32_t t=0;t<tracks;++t){auto& track=s.tracks[t];track.gain=.012f;
        if(t%4==2){track.drums.emplace();for(uint8_t pitch:{35,38,42,46})track.drums->pads.push_back({pitch,sample,{1,0,0,false,true,static_cast<uint32_t>(pitch>=42?1:0)}});}
        else if(t%4==3){track.sample=sample;track.rootNote=60;track.oneShot=true;}
        else{track.synth.waveform=static_cast<daw::Waveform>(1+(t/4)%3);track.synth.pulseWidth=t%2?.1f:.9f;track.synth.sustain=.8f;}
        for(int v=0;v<8;++v){const uint8_t pitch=t%4==2?static_cast<uint8_t>(v%2?35:38):t%4==3?60:static_cast<uint8_t>(48+v*3);s.notes.push_back({id++,0,s.lengthFrames,t,pitch,100});}
        if(t%4==2)for(uint64_t frame=24000;frame<s.lengthFrames;frame+=24000)s.notes.push_back({id++,frame,frame+240,t,static_cast<uint8_t>(frame%48000?46:42),80});
    }return s;
}
void lifecycle(){auto s=mixed(32);for(auto& t:s.tracks)t.gain=.001f;daw::EngineService e(s);daw::OutputDevice device;
    for(int cycle=0;cycle<10;++cycle){e.submit({daw::CommandType::Play});device.start(e,daw::DeviceMode::Native);std::this_thread::sleep_for(std::chrono::milliseconds(250));device.stop();if(device.errorCode())throw std::runtime_error("Native lifecycle device error");const auto blocks=e.telemetry().blocks;std::this_thread::sleep_for(std::chrono::milliseconds(20));if(e.telemetry().blocks!=blocks)throw std::runtime_error("Audio consumer survived stop/join");}
    std::cout<<"PASS 10 real native start/stop cycles; zero device errors; render counter remains stable after stop/join\n";
}
void run(uint32_t tracks,double seconds,bool native){
    auto spec=mixed(tracks);if(native)for(auto& t:spec.tracks)t.gain=.001f;auto start=Clock::now();daw::EngineService e(spec);
    const auto prepare=std::chrono::duration<double,std::milli>(Clock::now()-start).count();
    e.measureBlocks(static_cast<size_t>(std::ceil(seconds*48000/64))+4096);
    daw::Command loop{daw::CommandType::Loop};loop.enabled=true;loop.b=spec.lengthFrames;e.submit(loop);e.submit({daw::CommandType::Play});
    const auto rssLive=rss();uint64_t driverFailure=0,schedulerLate=0;int64_t observedOverloads=-1;double maxWakeMs=0;
    auto begin=Clock::now();auto next=begin;size_t blocks=0;std::array<float,512> out{};
    if(native){
#if defined(__APPLE__)
        OverloadMonitor monitor;overloads.store(0);
#endif
        daw::OutputDevice device;device.start(e,daw::DeviceMode::Native);begin=Clock::now();
        while(std::chrono::duration<double>(Clock::now()-begin).count()<seconds){std::this_thread::sleep_for(std::chrono::milliseconds(50));if(device.errorCode()){driverFailure=device.errorCode();break;}}
        device.stop();
#if defined(__APPLE__)
        if(monitor.registered)observedOverloads=static_cast<int64_t>(overloads.load());
#endif
    }else{
        while(std::chrono::duration<double>(Clock::now()-begin).count()<seconds){
            const double wake=std::chrono::duration<double,std::milli>(Clock::now()-next).count();maxWakeMs=std::max(maxWakeMs,wake);schedulerLate+=wake>5.333333;
            if(blocks%32==0){daw::MixSnapshot m;m.updateSynths=true;m.updateDrums=true;for(uint32_t t=0;t<tracks;++t){m.tracks[t].gain=spec.tracks[t].gain;m.synths[t]=spec.tracks[t].synth;m.synths[t].shape=blocks%64?.8f:1.f;if(spec.tracks[t].drums)for(size_t p=0;p<4;++p)m.drums[t].pads[p]=spec.tracks[t].drums->pads[p].parameters;}uint64_t serial=0;e.submitMix(m,serial);}
            e.render(out.data(),256);++blocks;next+=std::chrono::duration_cast<Clock::duration>(std::chrono::duration<double>(256./48000));std::this_thread::sleep_until(next);
        }
    }
    const double elapsed=std::chrono::duration<double>(Clock::now()-begin).count();
    const double audioSeconds=e.telemetry().renderedFrames/48000.;
    const bool continuous=elapsed>=seconds && elapsed<=seconds+1 && audioSeconds>=seconds-.2;
    if(e.telemetry().blocks!=e.measurements().size())throw std::runtime_error("Measurement buffer truncated; increase prepared capacity");
    std::vector<double> times,ratios;size_t over=0;for(const auto& x:e.measurements()){const double ms=x.nanoseconds/1e6;times.push_back(ms);ratios.push_back(ms/(x.frames/48.));over+=ms>x.frames/48.;}
    if(times.empty())throw std::runtime_error("No render measurements");std::sort(times.begin(),times.end());std::sort(ratios.begin(),ratios.end());const auto p99=(times.size()-1)*99/100;
    std::cout<<std::fixed<<std::setprecision(6)<<"{\"mode\":\""<<(native?"native_device":"headless_paced")<<"\",\"tracks\":"<<tracks<<",\"synths\":"<<tracks/2<<",\"drums\":"<<tracks/4<<",\"samplers\":"<<tracks/4<<",\"voices_per_synth\":8,\"sample_rate\":48000,\"internal_block_frames\":256,\"seconds\":"<<elapsed<<",\"blocks\":"<<times.size()<<",\"dsp_p95_ms\":"<<times[(times.size()-1)*95/100]<<",\"dsp_p99_ms\":"<<times[p99]<<",\"dsp_max_ms\":"<<times.back()<<",\"p99_budget_ratio\":"<<ratios[p99]<<",\"max_budget_ratio\":"<<ratios.back()<<",\"dsp_over_budget\":"<<over<<",\"wake_over_1_block\":"<<schedulerLate<<",\"max_wake_late_ms\":"<<maxWakeMs<<",\"valid_continuous_window\":"<<(continuous?"true":"false")<<",\"audio_seconds\":"<<audioSeconds<<",\"device_error\":"<<driverFailure<<",\"device_underruns\":null,\"coreaudio_overload_notifications\":"<<(observedOverloads<0?"null":std::to_string(observedOverloads))<<",\"track_gain\":"<<(native?.001:.012)<<",\"prepare_ms\":"<<prepare<<",\"live_rss_bytes\":"<<rssLive<<",\"final_rss_bytes\":"<<rss()<<",\"clipped_samples\":"<<e.telemetry().clippedSamples<<"}"<<std::endl;
    if(!continuous)throw std::runtime_error("Invalid continuous window: elapsed/audio duration mismatch; possible host suspension or severe scheduling gap");
    if(driverFailure||e.telemetry().clippedSamples)throw std::runtime_error("Device error or clipping");
}
}
int main(int argc,char** argv){try{if(argc==2&&std::string(argv[1])=="--lifecycle"){lifecycle();return 0;}if(argc<3)throw std::runtime_error("Usage: performance_probe TRACKS SECONDS [--native]");auto tracks=static_cast<uint32_t>(std::stoul(argv[1]));auto seconds=std::stod(argv[2]);if((tracks!=32&&tracks!=64)||!std::isfinite(seconds)||seconds<=0||seconds>900)throw std::runtime_error("Tracks 32/64; seconds 0..900");run(tracks,seconds,argc>3&&std::string(argv[3])=="--native");return 0;}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}}

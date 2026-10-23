#include "open_daw/engine.hpp"
#include "open_daw/service.hpp"
#include "open_daw/api.h"
#include <algorithm>
#include <cmath>
#include <chrono>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <limits>
#include <random>
#include <thread>
#include <vector>
#define CHECK(x) do { if (!(x)) throw std::runtime_error(std::string(__FILE__)+":"+std::to_string(__LINE__)+": " #x); } while (0)
namespace {
using namespace daw;
std::vector<float> render(SynthNode& synth, uint32_t frames, bool on=false, bool off=false, uint8_t pitch=69, uint64_t id=1) {
    AudioBlock out;out.allocate(frames);
    BlockNoteEvent e{0,pitch,127,on,id};
    if(on||off)synth.setBlockEvents({&e,1});
    synth.process({0,frames,48000},{},out);return out.left;
}
SynthConfig sustained(Waveform wave=Waveform::Sine){SynthConfig c;c.waveform=wave;c.attack=0;c.decay=0;c.sustain=1;c.release=0;return c;}
void envelopesAndSmoothing() {
    auto bank=std::make_shared<OscillatorBank>(48000);
    SynthNode a,b; a.setBank(bank);b.setBank(bank);
    SynthConfig old;old.attack=.1f;old.decay=.2f;old.release=.15f;
    a.configure(old);b.configure(old);a.prepare(48000,256);b.prepare(48000,256);
    CHECK(render(a,512,true)==render(b,512,true));
    auto next=old;next.attack=0;next.decay=0;next.sustain=0;next.release=0;
    a.applyConfig(next);
    CHECK(render(a,1024)==render(b,1024)); // captured attack / decay / sustain
    CHECK(render(a,8000,false,true)==render(b,8000,false,true)); // captured release, even during attack
    auto zero=render(a,1024,true,false,69,2);auto audible=render(b,1024,true,false,69,2);
    CHECK(std::all_of(zero.begin(),zero.end(),[](float v){return v==0;}));CHECK(zero!=audible);
    // An interrupted transition starts at its current mixture; endpoints are exact at 10ms.
    SynthNode live, sine, saw;
    for(auto* n:{&live,&sine,&saw}){n->setBank(bank);n->configure(sustained(n==&saw?Waveform::Saw:Waveform::Sine));n->prepare(48000,256);render(*n,1024,true);}
    live.applyConfig(sustained(Waveform::Saw));auto l=render(live,700),s=render(sine,700),w=render(saw,700);
    for(size_t i=0;i<l.size();++i){const float t=std::min(1.f,(i+1)/480.f);CHECK(std::abs(l[i]-(s[i]*(1-t)+w[i]*t))<3e-6f);}
    CHECK(std::equal(l.begin()+480,l.end(),w.begin()+480));
    // Shape and PWM smoothing preserve phase and converge in exactly the same duration.
    auto pulse=sustained(Waveform::Square);pulse.pulseWidth=.1f;pulse.shape=.25f;
    live.applyConfig(pulse);render(live,137);pulse.pulseWidth=.9f;pulse.shape=1;live.applyConfig(pulse);
    SynthNode target;target.setBank(bank);target.configure(pulse);target.prepare(48000,256);render(target,1024+700+137,true);
    auto moved=render(live,700),reference=render(target,700);CHECK(std::equal(moved.begin()+480,moved.end(),reference.begin()+480));
    live.reset();CHECK(render(live,1024)==std::vector<float>(1024));
    // Setup and runtime must quantize the original seconds at the target sample rate.
    for(double rate:{44100.,96000.}){
        auto rateBank=std::make_shared<OscillatorBank>(rate);SynthNode prepared,runtime;prepared.setBank(rateBank);runtime.setBank(rateBank);
        auto c=sustained();c.attack=.00001f;c.decay=.00003f;c.sustain=.2f;c.release=.00004f;
        prepared.configure(c);prepared.prepare(rate,256);runtime.prepare(rate,256);runtime.applyConfig(c);runtime.reset();
        CHECK(render(prepared,256,true)==render(runtime,256,true));CHECK(render(prepared,256,false,true)==render(runtime,256,false,true));
    }
    std::cout<<"PASS ADSR captured per voice, release during attack, exact 10ms waveform/shape/PWM ramps, reset\n";
}
void fullMidiRange() {
    for(double rate:{8000.,44100.,48000.,96000.}){
        auto bank=std::make_shared<OscillatorBank>(rate);
        for(auto wave:{Waveform::Sine,Waveform::Saw,Waveform::Square,Waveform::Triangle})for(float duty:{.05f,.5f,.95f}){
            SynthNode n;n.setBank(bank);auto c=sustained(wave);c.pulseWidth=duty;n.configure(c);n.prepare(rate,256);
            for(int pitch=0;pitch<128;++pitch){n.reset();auto v=render(n,1024,true,false,static_cast<uint8_t>(pitch));
                for(float x:v)CHECK(std::isfinite(x)&&std::abs(x)<.5f);
                if(440*std::pow(2.,(pitch-69)/12.)>=rate*.5)CHECK(std::all_of(v.begin(),v.end(),[](float x){return x==0;}));
            }
        }
    }
    // Both the explicit zero and maximum supported ADSR paths are finite.
    SynthConfig max;max.attack=10;max.decay=10;max.sustain=1;max.release=20;
    SynthNode n;n.configure(max);n.prepare(48000,256);render(n,256,true);for(float v:render(n,256,false,true))CHECK(std::isfinite(v));
    std::cout<<"PASS MIDI 0..127, all waveforms/duty limits, 8/44.1/48/96kHz, Nyquist silence and envelope limits\n";
}
od_synth_v1 raw(SynthConfig c){return {sizeof(od_synth_v1),1,static_cast<uint32_t>(c.waveform),c.shape,c.pulseWidth,c.attack,c.decay,c.sustain,c.release};}
od_track_v3 track(SynthConfig c){return {sizeof(od_track_v3),3,{sizeof(od_track_v2),2,{.5f,0,nullptr,60,0},0,0},raw(c)};}
void abiContracts(){
    static_assert(sizeof(od_track)==24 && sizeof(od_track_v2)==40 && sizeof(od_synth_v1)==36 && sizeof(od_track_v3)==88 && sizeof(od_parameters_v1)==52);
    od_engine* e=od_create(48000,256);CHECK(e);auto t=track(sustained());CHECK(od_load_v3(e,&t,1,nullptr,0,48000)==OD_OK);
    od_mix_status_v1 status{};CHECK(od_read_mix_status_v1(e,&status)==OD_OK);auto gen=status.generation;uint64_t serial=999;
    od_parameters_v1 p{{.5f,0,0,0},raw(sustained(Waveform::Saw))};
    CHECK(od_set_parameters_v1(e,gen+1,&p,1,&serial)==OD_INVALID);CHECK(serial==999);
    for(float bad:{-1.f,2.f,std::numeric_limits<float>::quiet_NaN(),std::numeric_limits<float>::infinity()}){auto invalid=p;invalid.synth.shape=bad;CHECK(od_set_parameters_v1(e,gen,&invalid,1,&serial)==OD_INVALID);CHECK(serial==999);}
    auto invalid=p;invalid.synth.version=2;CHECK(od_set_parameters_v1(e,gen,&invalid,1,&serial)==OD_INVALID);
    CHECK(od_set_parameters_v1(e,gen,nullptr,1,&serial)==OD_INVALID);CHECK(od_set_parameters_v1(e,gen,&p,0,&serial)==OD_INVALID);
    for(int i=0;i<8;++i)CHECK(od_set_parameters_v1(e,gen,&p,1,&serial)==OD_OK);
    CHECK(serial==8);p.synth=raw(sustained(Waveform::Triangle));CHECK(od_set_parameters_v1(e,gen,&p,1,&serial)==OD_QUEUE_FULL);CHECK(serial==8);
    std::vector<float> audio(4096);CHECK(od_stop(e)==OD_OK);CHECK(od_render(e,audio.data(),256)==OD_OK);
    CHECK(od_read_mix_status_v1(e,&status)==OD_OK);CHECK(status.applied==8);
    // A subsequent legacy mix must preserve the accepted saw, even in the same callback batch.
    od_track_mix_v1 mix{.5f,0,0,0};CHECK(od_set_mix_v1(e,gen,&mix,1,&serial)==OD_OK);
    CHECK(od_live_note(e,0,LiveIdBit|1,69,127,1)==OD_OK);CHECK(od_render(e,audio.data(),2048)==OD_OK);
    od_engine* ref=od_create(48000,256);auto saw=track(sustained(Waveform::Saw));CHECK(od_load_v3(ref,&saw,1,nullptr,0,48000)==OD_OK);
    CHECK(od_live_note(ref,0,LiveIdBit|1,69,127,1)==OD_OK);std::vector<float> expected(4096);CHECK(od_render(ref,expected.data(),2048)==OD_OK);CHECK(audio==expected);
    // Coalescing a full parameter snapshot followed by a legacy mixer update keeps both.
    p.synth=raw(sustained(Waveform::Triangle));CHECK(od_set_parameters_v1(e,gen,&p,1,&serial)==OD_OK);
    CHECK(od_set_mix_v1(e,gen,&mix,1,&serial)==OD_OK);CHECK(od_stop(e)==OD_OK);CHECK(od_render(e,audio.data(),256)==OD_OK);
    auto triangle=track(sustained(Waveform::Triangle));CHECK(od_load_v3(ref,&triangle,1,nullptr,0,48000)==OD_OK);
    CHECK(od_live_note(e,0,LiveIdBit|2,69,127,1)==OD_OK);CHECK(od_live_note(ref,0,LiveIdBit|2,69,127,1)==OD_OK);
    CHECK(od_render(e,audio.data(),2048)==OD_OK);CHECK(od_render(ref,expected.data(),2048)==OD_OK);CHECK(audio==expected);
    auto wrong=t;wrong.synth.pulse_width=0;CHECK(od_load_v3(e,&wrong,1,nullptr,0,48000)==OD_INVALID);
    CHECK(od_read_mix_status_v1(e,&status)==OD_OK);CHECK(status.generation==gen);
    CHECK(od_load_v3(e,&t,1,nullptr,0,48000)==OD_OK);CHECK(od_set_parameters_v1(e,gen,&p,1,&serial)==OD_INVALID);
    od_destroy(e);od_destroy(ref);std::cout<<"PASS additive ABI sizes, strict validation, generations, overflow, Stop and legacy mix preserve synth\n";
}
std::vector<float> session(const SessionSpec& spec,const std::vector<uint32_t>& sizes){
    EngineService e(spec);CHECK(e.submit({CommandType::Play}));std::vector<float> v(spec.lengthFrames*2);uint64_t done=0;size_t i=0;
    while(done<spec.lengthFrames){auto n=static_cast<uint32_t>(std::min<uint64_t>(sizes[i++%sizes.size()],spec.lengthFrames-done));e.render(v.data()+2*done,n);done+=n;}return v;
}
void partitionAndIsolation(){
    SessionSpec spec;spec.lengthFrames=24000;spec.tracks.resize(3);spec.tracks[0].synth=sustained(Waveform::Saw);spec.tracks[1].synth=sustained(Waveform::Square);spec.tracks[2].synth=sustained(Waveform::Triangle);
    spec.notes={{1,0,18001,0,60,80},{2,124,18591,1,72,85},{3,511,19201,2,48,90},{4,8000,20001,0,60,70}};
    const auto ref=session(spec,{256});for(uint32_t n:{64u,128u,511u,1024u})CHECK(session(spec,{n})==ref);
    std::mt19937 random(2026);std::vector<uint32_t> parts;for(int i=0;i<25;++i)parts.push_back(1+random()%1024);CHECK(session(spec,parts)==ref);
    spec.tracks[0].mute=true;auto isolated=session(spec,{256});spec.tracks[0].synth=sustained(Waveform::Triangle);spec.notes[0].pitch=127;CHECK(session(spec,{256})==isolated);
    std::cout<<"PASS 3 independent synths, overlapping IDs, 5 fixed +25 random export partitions, muted instance isolation\n";
}
void concurrentParameters(){
    SessionSpec s;s.lengthFrames=48000;s.notes={{1,0,47000,0,69,100}};EngineService e(s);e.submit({CommandType::Play});std::atomic<bool> done{false};
    std::thread audio([&]{std::vector<float> out(512);for(int i=0;i<2000;++i)e.render(out.data(),256);done.store(true);});
    uint64_t serial=0;for(int i=0;i<2000&&!done.load();++i){MixSnapshot p;p.updateSynths=true;p.synths[0].waveform=static_cast<Waveform>(i%4);p.synths[0].pulseWidth=i%2?.05f:.95f;e.submitMix(p,serial);(void)e.mixApplied();}
    audio.join();std::cout<<"PASS concurrent POD synth snapshots / telemetry\n";
}
void benchmark(){
    using Clock=std::chrono::steady_clock;const auto start=Clock::now();
    SessionSpec spec;spec.lengthFrames=48000*60;spec.tracks.resize(16);uint64_t id=1;
    for(uint32_t track=0;track<16;++track){spec.tracks[track].gain=.03f;spec.tracks[track].synth=sustained(static_cast<Waveform>(track%4));
        for(int v=0;v<8;++v)spec.notes.push_back({id++,0,spec.lengthFrames-1,track,static_cast<uint8_t>(48+v*3),100});}
    EngineService engine(spec);const double prepare=std::chrono::duration<double,std::milli>(Clock::now()-start).count();
    engine.submit({CommandType::Play});std::vector<float> out(512);std::vector<double> timings;timings.reserve(5000);
    for(int b=0;b<5000;++b){
        if(b%4==0){MixSnapshot p;p.updateSynths=true;for(uint32_t t=0;t<16;++t){p.tracks[t].gain=.03f;p.synths[t]=spec.tracks[t].synth;p.synths[t].waveform=static_cast<Waveform>((t+b/4)%4);p.synths[t].shape=(b%100)/100.f;p.synths[t].pulseWidth=b%8?.1f:.9f;}
            uint64_t serial=0;CHECK(engine.submitMix(p,serial));}
        const auto begin=Clock::now();engine.render(out.data(),256);timings.push_back(std::chrono::duration<double,std::milli>(Clock::now()-begin).count());
        for(float x:out)CHECK(std::isfinite(x));
    }
    std::sort(timings.begin(),timings.end());std::cout<<"16 tracks x 8 held voices, 5000 x 256 frames @48kHz, full synth snapshots every 4 blocks\n"
        <<"prepare "<<prepare<<" ms; callback p95 "<<timings[4749]<<" ms; p99 "<<timings[4949]<<" ms; max "<<timings.back()<<" ms\n"
        <<"Immutable bank per sample rate "<<(128.0*2*(OscillatorBank::TableSize+1)*sizeof(float)/(1024*1024))<<" MiB, shared by tracks/export.\n"
        <<"Isolated DSP benchmark; no physical device or scheduler/underrun certification.\n";
}
void motion(const std::filesystem::path& path){
    SessionSpec spec;spec.lengthFrames=48000*7;spec.tracks[0].gain=.7f;spec.tracks[0].synth.waveform=Waveform::Square;
    spec.tracks[0].synth.decay=0;spec.tracks[0].synth.sustain=1;spec.tracks[0].synth.release=.02f;
    for(uint64_t second=0;second<7;++second)spec.notes.push_back({second+1,second*48000,second*48000+45000,0,static_cast<uint8_t>(45+second*12),100});
    EngineService engine(spec);engine.submit({CommandType::Play});WavStreamWriter writer(path,48000,spec.lengthFrames);std::vector<float> out(512);
    for(uint64_t frame=0;frame<spec.lengthFrames;){
        MixSnapshot snapshot;snapshot.updateSynths=true;snapshot.tracks[0].gain=.7f;snapshot.synths[0]=spec.tracks[0].synth;
        snapshot.synths[0].pulseWidth=.5f+.4f*static_cast<float>(std::sin(6.283185307179586*frame/48000*.5));
        snapshot.synths[0].waveform=static_cast<Waveform>(1+(frame/48000)%3);uint64_t serial=0;CHECK(engine.submitMix(snapshot,serial));
        const auto n=static_cast<uint32_t>(std::min<uint64_t>(256,spec.lengthFrames-frame));engine.render(out.data(),n);
        for(float v:out)CHECK(std::isfinite(v));writer.write({out.data(),size_t{n}*2});frame+=n;
    }
    writer.finish();CHECK(engine.telemetry().clippedSamples==0);std::cout<<"Rendered stepped MIDI 45..117 sweep, saw/PWM/triangle changes and 0.1..0.9 animated PWM; finite, no clipping. Listening on a physical device still required.\n";
}
void spectra(const std::filesystem::path& dir){
    std::filesystem::create_directories(dir);std::ofstream metadata(dir/"cases.csv");metadata<<"file,rate,nominal_rate,frequency,bin,wave,duty,frames\n";
    for(int rate:{44100,48000,96000})for(int pitch:{45,69,93,105,117}){
        const double hz=440*std::pow(2.,(pitch-69)/12.);const auto n=static_cast<uint32_t>(rate);const int bin=static_cast<int>(hz);const double coherentRate=rate;
        auto bank=std::make_shared<OscillatorBank>(coherentRate);
        for(int w:{1,2,3})for(float duty:{.1f,.5f,.9f}){
            if(w!=2&&duty!=.5f)continue;
            auto c=sustained(static_cast<Waveform>(w));c.pulseWidth=duty;SynthNode synth;synth.setBank(bank);synth.configure(c);synth.prepare(coherentRate,256);
            auto v=render(synth,n,true,false,static_cast<uint8_t>(pitch));
            const std::string name=std::to_string(rate)+"-"+std::to_string(pitch)+"-"+std::to_string(w)+"-"+std::to_string(duty)+".f32";
            std::ofstream f(dir/name,std::ios::binary);f.write(reinterpret_cast<const char*>(v.data()),v.size()*sizeof(float));
            metadata.precision(17);metadata<<name<<','<<coherentRate<<','<<rate<<','<<hz<<','<<bin<<','<<w<<','<<duty<<','<<n<<'\n';
        }
    }
}
}
int main(int argc,char** argv){try{if(argc==2&&std::string(argv[1])=="--benchmark"){benchmark();return 0;}if(argc==3&&std::string(argv[1])=="--motion"){motion(argv[2]);return 0;}if(argc==3&&std::string(argv[1])=="--spectra"){spectra(argv[2]);return 0;}
    envelopesAndSmoothing();fullMidiRange();abiContracts();partitionAndIsolation();concurrentParameters();return 0;
}catch(const std::exception& e){std::cerr<<"FAIL "<<e.what()<<'\n';return 1;}}

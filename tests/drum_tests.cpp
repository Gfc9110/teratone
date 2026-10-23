#include "open_daw/drums.hpp"
#include "open_daw/service.hpp"
#include "open_daw/api.h"
#include <cmath>
#include <iostream>
#include <limits>
#include <random>
#include <thread>
#include <chrono>
#include <algorithm>
#define CHECK(x) do{if(!(x))throw std::runtime_error(std::string(__FILE__)+":"+std::to_string(__LINE__)+": " #x);}while(0)
using namespace daw;
namespace {
std::shared_ptr<SampleData> sample(float value=.25f,size_t frames=16000){auto s=std::make_shared<SampleData>();s->stereo.assign(frames*2,value);return s;}
DrumKit kit(){auto a=sample();return {{{35,a,{}},{46,a,{1,0,0,false,true,1}},{42,sample(0),{1,0,0,false,true,1}},{60,a,{}}}};}
AudioBlock block(DrumNode& n,uint32_t frames,std::span<const BlockNoteEvent> events={}){AudioBlock out;out.allocate(frames);n.setBlockEvents(events);n.process({0,frames,48000},{},out);return out;}
float energy(const AudioBlock& b,size_t start=0){float e=0;for(size_t i=start;i<b.left.size();++i)e+=std::abs(b.left[i])+std::abs(b.right[i]);return e;}
void mappingVelocity(){
    auto k=kit();DrumNode a(k),b(k);a.prepare(48000,256);b.prepare(48000,256);
    BlockNoteEvent on{13,35,127,true,1},other{13,60,127,true,1};auto x=block(a,1024,{&on,1}),y=block(b,1024,{&other,1});CHECK(x.left==y.left&&x.right==y.right);
    CHECK(x.left[12]==0&&x.left[13]>0);for(int velocity:{32,64,100,127}){a.reset();on.offset=0;on.velocity=static_cast<uint8_t>(velocity);auto out=block(a,1024,{&on,1});CHECK(std::abs(out.left[1023]-.25f*.70710678f*velocity/127.f)<1e-6f);}
    a.reset();on.note=99;CHECK(energy(block(a,1024,{&on,1}))==0);k.pads[0].sample.reset();DrumNode empty(k);empty.prepare(48000,256);on.note=35;CHECK(energy(block(empty,1024,{&on,1}))==0);
    std::cout<<"PASS MIDI lookup, empty/unmapped silence, exact event offset, velocity and no MIDI pitch transposition\n";
}
void modesAndChoke(){
    auto k=kit();DrumNode a(k);a.prepare(48000,256);BlockNoteEvent on{0,46,127,true,1};block(a,1024,{&on,1});BlockNoteEvent off{0,46,0,false,1};CHECK(energy(block(a,512,{&off,1}))>0);
    DrumControls params;for(size_t i=0;i<k.pads.size();++i)params.pads[i]=k.pads[i].parameters;params.pads[1].oneShot=false;a.applyParameters(params);
    CHECK(energy(block(a,512,{&off,1}))>0); // existing one-shot retains its mode
    a.reset();block(a,512,{&on,1});auto released=block(a,512,{&off,1});CHECK(energy(released)>0&&energy(released,240)==0);
    a.reset();block(a,1024,{&on,1});BlockNoteEvent closed{31,42,127,true,2};auto choked=block(a,512,{&closed,1});CHECK(choked.left[30]>0&&energy(choked,31+240)==0);
    DrumNode isolated(k);isolated.prepare(48000,256);block(isolated,1024,{&on,1});CHECK(energy(block(isolated,512))>0);
    a.reset();BlockNoteEvent simultaneous[]{{0,46,127,true,3},{0,42,127,true,4}};CHECK(energy(block(a,512,simultaneous))==0);
    auto shortKit=kit();shortKit.pads[1].sample=sample(.25f,400);DrumNode shortTail(shortKit);shortTail.prepare(48000,256);block(shortTail,300,{&on,1});closed.offset=0;auto nearEnd=block(shortTail,600,{&closed,1});CHECK(energy(nearEnd,240)==0);
    std::cout<<"PASS captured gated/one-shot modes, 5ms local choke, simultaneous event ordering, independent kits\n";
}
void parametersAndSteal(){
    auto k=kit();DrumNode a(k);a.prepare(48000,256);BlockNoteEvent on{0,35,127,true,1};block(a,1024,{&on,1});DrumControls c;for(size_t i=0;i<k.pads.size();++i)c.pads[i]=k.pads[i].parameters;
    c.pads[0].mute=true;a.applyParameters(c);auto fade=block(a,512);CHECK(energy(fade)>0&&energy(fade,240)==0);
    c.pads[0].mute=false;c.pads[0].pan=-1;a.applyParameters(c);auto left=block(a,8192);CHECK(std::abs(left.right.back())<1e-6f&&left.left.back()>.2f);
    a.reset();std::array<BlockNoteEvent,64> notes;for(size_t i=0;i<notes.size();++i)notes[i]={0,35,127,true,i+1};block(a,1024,notes);CHECK(a.status().active==64);
    auto negative=k;negative.pads[3].sample=sample(-.25f);DrumNode steal(negative);steal.prepare(48000,256);auto before=block(steal,1024,notes);BlockNoteEvent hit{0,60,127,true,100};auto after=block(steal,512,{&hit,1});
    CHECK(std::abs(after.left.front()-before.left.back())<.002f);CHECK(steal.status().active<=64);
    for(int i=0;i<200;++i){hit.id++;auto b=block(steal,1,{&hit,1});CHECK(std::isfinite(b.left[0]));CHECK(steal.status().active<=64);}
    auto shortMute=kit();shortMute.pads[0].sample=sample(.25f,400);DrumNode muteEnd(shortMute);muteEnd.prepare(48000,256);block(muteEnd,300,{&on,1});c.pads[0].mute=true;muteEnd.applyParameters(c);CHECK(energy(block(muteEnd,600),240)==0);
    auto invalid=k;invalid.pads[1].midiNote=35;bool threw=false;try{DrumNode bad(invalid);}catch(const std::invalid_argument&){threw=true;}CHECK(threw);
    std::cout<<"PASS realtime pad mute/pan, exact silence, 64-slot deterministic stealing/declick, bounded retrigger storm\n";
}
std::vector<float> render(EngineService& e,uint32_t frames){std::vector<float> out(frames*2);e.render(out.data(),frames);return out;}
void liveScopeAndPartitions(){
    SessionSpec s;s.lengthFrames=24000;s.tracks[0].drums=kit();s.notes={{1,0,16000,0,35,100}};EngineService a(s),b(s);a.submit({CommandType::Play});b.submit({CommandType::Play});
    Command live{CommandType::NoteOn};live.id=LiveIdBit|1;live.pitch=35;CHECK(a.submit(live));CHECK(a.cancelLive());CHECK(render(a,1024)==render(b,1024));
    live.id=LiveIdBit|2;a.submit(live);render(a,1024);render(b,1024);live.type=CommandType::NoteOff;a.submit(live);render(a,512);render(b,512);CHECK(a.cancelLive());render(a,256);render(b,256);CHECK(render(a,2048)==render(b,2048));CHECK(a.telemetry().playing&&a.telemetry().position==b.telemetry().position);
    // A fresh live note after cancellation survives the old command backlog.
    a.cancelLive();live.type=CommandType::NoteOn;live.id=LiveIdBit|3;a.submit(live);CHECK(render(a,1024)!=render(b,1024));
    SessionSpec score;score.tracks.resize(2);score.tracks[0].drums=kit();score.tracks[1].drums=kit();score.lengthFrames=24000;
    score.notes={{1,0,500,0,35,100},{2,15,700,0,46,100},{3,511,900,0,42,90},{4,80,1000,1,46,80},{5,1200,1500,1,60,50}};
    EngineService reference(score);reference.submit({CommandType::Play});const auto expected=render(reference,24000);
    for(uint32_t n:{64u,128u,256u,511u,1024u}){EngineService e(score);e.submit({CommandType::Play});std::vector<float> out(48000);for(uint32_t pos=0;pos<24000;){auto count=std::min(n,24000-pos);e.render(out.data()+2*pos,count);pos+=count;}CHECK(out==expected);}
    EngineService random(score);random.submit({CommandType::Play});std::mt19937 rng(35);std::vector<float> out(48000);for(uint32_t pos=0;pos<24000;){auto count=std::min<uint32_t>(1+static_cast<uint32_t>(rng()%1024),24000-pos);random.render(out.data()+2*pos,static_cast<uint32_t>(count));pos+=static_cast<uint32_t>(count);}CHECK(out==expected);
    std::cout<<"PASS priority one-shot preview cancellation, stale/fresh epochs, score continuity and fixed/random partition parity\n";
}
void nativeConcurrency(){
    SessionSpec s;s.tracks[0].drums=kit();s.lengthFrames=48000;EngineService e(s);std::atomic<bool> done{false};
    std::thread audio([&]{std::array<float,512> out{};for(int i=0;i<3000;++i)e.render(out.data(),256);done.store(true);});
    for(uint64_t i=1;i<2000&&!done.load();++i){Command c{CommandType::NoteOn};c.id=LiveIdBit|i;c.pitch=35;e.submit(c);if(i%5==0)e.cancelLive();MixSnapshot m;m.updateDrums=true;m.drums[0].pads[0].pan=i%2?-1:1;uint64_t serial=0;e.submitMix(m,serial);DrumStatus status;CHECK(e.drumStatus(0,status));CHECK(status.active<=64);}
    audio.join();std::cout<<"PASS concurrent drum controls, live cancellation and trigger/voice meters\n";
}
void benchmark(){
    using Clock=std::chrono::steady_clock;
    for(uint32_t frames:{64u,128u,256u,1024u}){
        SessionSpec s;s.maxFrames=frames;s.lengthFrames=48000*60;s.tracks.resize(16);auto held=sample(.02f,1200000);
        uint64_t id=1;
        for(uint32_t t=0;t<16;++t){s.tracks[t].gain=.03f;s.tracks[t].drums=DrumKit{{{35,held,{}}}};for(int v=0;v<8;++v)s.notes.push_back({id++,0,48000*60,t,35,100});}
        auto prepared=Clock::now();EngineService e(s);const auto prepareMs=std::chrono::duration<double,std::milli>(Clock::now()-prepared).count();e.submit({CommandType::Play});
        std::vector<float> out(frames*2);for(int i=0;i<100;++i)e.render(out.data(),frames);
        std::vector<double> times;times.reserve(1000);
        for(int i=0;i<1000;++i){if(i%16==0){MixSnapshot m;m.updateDrums=true;for(int t=0;t<16;++t){m.tracks[t].gain=.03f;m.drums[t].pads[0].pan=i%32?-.5f:.5f;}uint64_t serial=0;e.submitMix(m,serial);}
            const auto begin=Clock::now();e.render(out.data(),frames);times.push_back(std::chrono::duration<double,std::milli>(Clock::now()-begin).count());}
        DrumStatus status;CHECK(e.drumStatus(0,status)&&status.active==8);CHECK(e.telemetry().clippedSamples==0);
        std::sort(times.begin(),times.end());const double deadline=frames/48.;
        std::cout<<"DRUM BENCH 16 tracks x 8 sustained voices, 48kHz, frames="<<frames<<" n=1000 prepare_ms="<<prepareMs<<" p95_ms="<<times[949]<<" p99_ms="<<times[989]<<" max_ms="<<times.back()<<" deadline_ms="<<deadline<<" p99_budget_pct="<<times[989]/deadline*100<<'\n';
    }
}
}
int main(int argc,char**){try{if(argc>1){benchmark();return 0;}mappingVelocity();modesAndChoke();parametersAndSteal();liveScopeAndPartitions();nativeConcurrency();return 0;}catch(const std::exception& e){std::cerr<<"FAIL "<<e.what()<<'\n';return 1;}}

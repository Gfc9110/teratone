#include "open_daw/service.hpp"
#include "open_daw/api.h"
#include <algorithm>
#include <iostream>
#include <random>
#include <thread>
#include <stdexcept>
#define CHECK(x) do{if(!(x))throw std::runtime_error(#x);}while(0)
static_assert(sizeof(daw::EngineService)<96*1024,"Prepared storage must not balloon Windows stacks");
namespace {
daw::SessionSpec score(){daw::SessionSpec s;s.tracks.resize(daw::MaxTracks);s.lengthFrames=8192;uint64_t id=1;auto sample=std::make_shared<daw::SampleData>();sample->stereo.assign(16384,.05f);
    for(uint32_t t=0;t<daw::MaxTracks;++t){auto& track=s.tracks[t];track.gain=.005f;if(t%4==2)track.drums=daw::DrumKit{{{35,sample,{}}}};else if(t%4==3)track.sample=sample;
        for(int v=0;v<8;++v)s.notes.push_back({id++,static_cast<uint64_t>(v*7),6000,t,static_cast<uint8_t>(t%4==2?35:48+v),100});}return s;}
void partitionsAndPriority(){auto s=score();daw::EngineService a(s);a.submit({daw::CommandType::Play});std::vector<float> expected(16384);a.render(expected.data(),8192);
    for(uint32_t seed=0;seed<25;++seed){daw::EngineService b(s);b.submit({daw::CommandType::Play});std::vector<float> out(16384);std::mt19937 rng(seed);for(uint32_t pos=0;pos<8192;){auto n=std::min<uint32_t>(8192-pos,1+static_cast<uint32_t>(rng()%1024));b.render(out.data()+2*pos,n);pos+=n;}CHECK(out==expected);}
    daw::EngineService e(s);for(size_t i=0;i<1024;++i){daw::Command c{daw::CommandType::NoteOn};c.track=63;c.id=daw::LiveIdBit|(i+1);c.pitch=60;CHECK(e.submit(c));}
    daw::MixSnapshot mix;for(uint32_t t=0;t<64;++t)mix.tracks[t].gain=.01f;uint64_t serial=0;CHECK(e.submitMix(mix,serial));e.stop();std::array<float,512> out{};e.render(out.data(),256);CHECK(!e.telemetry().playing);CHECK(std::all_of(out.begin(),out.end(),[](float x){return x==0;}));CHECK(e.mixApplied()==serial);
    std::cout<<"PASS 64 mixed tracks / 512 voices, 25 bit-identical partitions; full note queue Stop in one block retains mix\n";
}
void concurrent64(){auto s=score();daw::EngineService e(s);std::atomic<bool> done{false};std::thread audio([&]{std::array<float,512> out{};for(int i=0;i<3000;++i)e.render(out.data(),256);done.store(true);});
    for(int i=0;i<1000&&!done.load();++i){daw::MixSnapshot mix;mix.updateSynths=true;mix.updateDrums=true;for(uint32_t t=0;t<64;++t){mix.tracks[t].gain=.005f;mix.synths[t].shape=i%2?.2f:.8f;}uint64_t serial=0;e.submitMix(mix,serial);daw::Command c{daw::CommandType::NoteOn};c.track=63;c.pitch=60;c.id=daw::LiveIdBit|static_cast<uint64_t>(i+1);e.submit(c);if(i%3==0)e.cancelLive();if(i%17==0)e.stop();}
    audio.join();std::cout<<"PASS concurrent 64-track full parameter snapshots, last-slot notes, preview cancellation and Stop\n";}
void abiBoundary(){auto* e=od_create(48000,256);CHECK(e);std::vector<od_track> tracks(65,od_track{.01f,0,nullptr,60,0});od_note note{1,0,4000,63,60,100};CHECK(od_track_capacity()==64);CHECK(od_load(e,tracks.data(),64,&note,1,8192)==OD_OK);CHECK(od_play(e)==OD_OK);std::array<float,512> out{};CHECK(od_render(e,out.data(),256)==OD_OK);CHECK(std::any_of(out.begin(),out.end(),[](float x){return x!=0;}));od_mix_status_v1 before{},after{};od_read_mix_status_v1(e,&before);CHECK(od_load(e,tracks.data(),65,&note,1,8192)==OD_INVALID);od_read_mix_status_v1(e,&after);CHECK(before.generation==after.generation);od_telemetry t{};od_read_telemetry(e,&t);CHECK(t.playing&&t.position==256);
    for(int i=0;i<32;++i)CHECK(od_load(e,tracks.data(),64,&note,1,8192)==OD_OK); // repeated replacements exercise control-side page release
    CHECK(od_gain_pan(e,63,.02f,-1)==OD_OK);CHECK(od_gain_pan(e,64,.02f,0)==OD_INVALID);od_destroy(e);std::cout<<"PASS old ABI layouts support track 64; track 65 rejected without replacing playback\n";}
}
int main(){try{partitionsAndPriority();concurrent64();abiBoundary();std::cout<<"EngineService stack bytes="<<sizeof(daw::EngineService)<<"; event heap bytes="<<sizeof(daw::BlockNoteEvent)*daw::BlockEventCapacity*64<<"; mixer queue heap bytes="<<sizeof(daw::SpscQueue<daw::MixSnapshot,9>)<<'\n';return 0;}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}}

#include "open_daw/drums.hpp"
#include <algorithm>
#include <cmath>
#include <bit>
namespace daw {
namespace { constexpr float PiOver4 = .7853981633974483096f; }
bool PadParameters::valid() const noexcept {
    return std::isfinite(gain) && gain >= 0 && gain <= 4 && std::isfinite(pan) && pan >= -1 && pan <= 1 &&
        std::isfinite(tune) && tune >= -24 && tune <= 24 && chokeGroup <= 16;
}
DrumNode::DrumNode(const DrumKit& kit) : padCount_(kit.pads.size()) {
    if (!padCount_ || padCount_ > MaxDrumPads) throw std::invalid_argument("Drum kit requires 1..16 pads");
    lookup_.fill(-1);
    for (size_t i = 0; i < padCount_; ++i) {
        const auto& source = kit.pads[i];
        if (source.midiNote > 127 || lookup_[source.midiNote] >= 0 || !source.parameters.valid())
            throw std::invalid_argument("Invalid drum parameters or duplicate MIDI mapping");
        if (source.sample) {
            if (!source.sample->frames() || source.sample->stereo.size() % 2 || source.sample->sampleRate < 8000 || source.sample->sampleRate > 384000)
                throw std::invalid_argument("Invalid drum sample");
            for (float value : source.sample->stereo) if (!std::isfinite(value)) throw std::invalid_argument("Non-finite drum sample");
        }
        lookup_[source.midiNote] = static_cast<int8_t>(i);
        pads_[i].sample = source.sample; pads_[i].parameters = source.parameters;
    }
}
void DrumNode::prepare(double rate, uint32_t) {
    if (!std::isfinite(rate) || rate < 8000 || rate > 384000) throw std::invalid_argument("Invalid drum sample rate");
    sampleRate_ = rate; attackFrames_ = std::max(1u, static_cast<uint32_t>(std::round(rate*.001)));
    releaseFrames_ = std::max(1u, static_cast<uint32_t>(std::round(rate*.005)));
    smoothing_ = 1 - std::exp(-1.f/static_cast<float>(rate*.005)); reset();
}
void DrumNode::reset() noexcept {
    voices_.fill(Voice{}); age_ = 0; blockEvents_ = {};active_.store(0,std::memory_order_relaxed);
    for (size_t i=0;i<padCount_;++i) {
        auto& p = pads_[i]; const auto& c = p.parameters;
        p.left = p.targetLeft = c.gain * std::cos((c.pan+1)*PiOver4);
        p.right = p.targetRight = c.gain * std::sin((c.pan+1)*PiOver4);
        p.gate = c.mute ? 0 : 1; p.gateLeft = 0; p.gateStep = 0;
    }
}
void DrumNode::applyParameters(const DrumControls& controls) noexcept {
    for (size_t i=0;i<padCount_;++i) {
        auto& p = pads_[i]; const auto& c = controls.pads[i];
        if (p.parameters.gain != c.gain || p.parameters.pan != c.pan) {
            p.targetLeft = c.gain * std::cos((c.pan+1)*PiOver4); p.targetRight = c.gain * std::sin((c.pan+1)*PiOver4);
        }
        if (p.parameters.mute != c.mute) { p.gateLeft = releaseFrames_; p.gateStep = ((c.mute ? 0.f : 1.f)-p.gate)/p.gateLeft; }
        p.parameters = c; // tune, oneShot and choke are captured by new voices
    }
}
float DrumNode::tailGate(uint32_t mask) const noexcept {
    float gate=1;while(mask){const auto index=std::countr_zero(mask);gate=std::min(gate,pads_[index].gate);mask&=mask-1;}return gate;
}
void DrumNode::startTail(Voice& v,float left,float right,uint32_t mask,uint32_t frames) noexcept {
    const float gate=std::max(.000001f,tailGate(mask));v.tailL=left/gate;v.tailR=right/gate;v.tailMask=mask;v.tailLength=std::max(1u,frames);v.tailLeft=(left!=0 || right!=0)?v.tailLength:0;
}
void DrumNode::release(Voice& voice) noexcept {
    if (voice.active && !voice.releasing) { voice.releasing = true; voice.releaseLeft = releaseFrames_; voice.releaseStart = voice.envelope; }
}
DrumStatus DrumNode::status() const noexcept {
    DrumStatus s;s.padCount=static_cast<uint32_t>(padCount_);s.active=active_.load(std::memory_order_relaxed);
    for(size_t i=0;i<padCount_;++i)s.triggers[i]=triggers_[i].load(std::memory_order_relaxed);return s;
}
void DrumNode::cancelLive() noexcept { for (auto& v:voices_) if (v.id >> 63) release(v); }
void DrumNode::event(const BlockNoteEvent& e) noexcept {
    if (!e.on || !e.velocity) {
        for (auto& v:voices_) if (!v.oneShot && (e.id ? e.id==v.id : lookup_[e.note]>=0 && v.pad==static_cast<size_t>(lookup_[e.note]))) release(v);
        return;
    }
    const auto index = lookup_[e.note]; if (index < 0 || !pads_[index].sample) return;
    triggers_[index].fetch_add(1,std::memory_order_relaxed);
    const auto& p = pads_[index];
    if (p.parameters.chokeGroup) for (auto& v:voices_) if (v.choke == p.parameters.chokeGroup) release(v);
    auto* dest = &voices_[0];
    for (auto& v:voices_) {
        if (!v.active && !v.tailLeft) { dest = &v; break; }
        const auto weak = [](const Voice& x) { return std::abs(x.lastL)+std::abs(x.lastR); };
        if ((v.releasing && !dest->releasing) || (v.releasing == dest->releasing && (weak(v)<weak(*dest) || (weak(v)==weak(*dest) && v.age<dest->age)))) dest=&v;
    }
    // Stealing holds the previous output and fades it, inside the same fixed slot.
    // Repeated stealing captures the combined output; no overflow tail pool or delayed trigger.
    const float oldL=dest->lastL,oldR=dest->lastR;const uint32_t mask=(1u<<dest->pad)|(dest->tailLeft ? dest->tailMask : 0);*dest=Voice{};
    startTail(*dest,oldL,oldR,mask,releaseFrames_);
    dest->attackLength=dest->tailLeft ? releaseFrames_ : attackFrames_;
    dest->pad=static_cast<size_t>(index);dest->id=e.id;dest->age=++age_;dest->velocity=e.velocity/127.f;
    dest->increment=p.sample->sampleRate/sampleRate_*std::pow(2.,p.parameters.tune/12.);
    dest->choke=p.parameters.chokeGroup;dest->oneShot=p.parameters.oneShot;dest->active=true;
}
void DrumNode::process(const ProcessContext& ctx,std::span<const AudioBlock* const>,AudioBlock& output) noexcept {
    size_t next=0;
    for (uint32_t frame=0;frame<ctx.frames;++frame) {
        while(next<blockEvents_.size() && blockEvents_[next].offset<=frame) event(blockEvents_[next++]);
        for(size_t i=0;i<padCount_;++i) {
            auto& p=pads_[i];p.left+=smoothing_*(p.targetLeft-p.left);p.right+=smoothing_*(p.targetRight-p.right);
            if(p.gateLeft) {p.gate+=p.gateStep;if(--p.gateLeft==0)p.gate=p.parameters.mute?0:1;}
        }
        float left=0,right=0;
        for(auto& v:voices_) {
            float l=0,r=0;
            if(v.active) {
                const auto& p=pads_[v.pad];const auto& sample=*p.sample;
                if(v.position>=sample.frames()) {
                    const auto mask=(1u<<v.pad)|(v.tailLeft ? v.tailMask : 0);startTail(v,v.lastL,v.lastR,mask,v.releasing ? v.releaseLeft : releaseFrames_);v.active=false;
                } else {
                    if(v.releasing) {v.envelope=v.releaseStart*static_cast<float>(v.releaseLeft)/releaseFrames_;if(--v.releaseLeft==0){v.envelope=0;v.active=false;}}
                    else {if(v.attack<v.attackLength)++v.attack;v.envelope=static_cast<float>(v.attack)/v.attackLength;}
                    const auto a=static_cast<size_t>(v.position),b=std::min(a+1,sample.frames()-1);const float f=static_cast<float>(v.position-a);
                    const float gain=v.velocity*v.envelope*p.gate;
                    l=(sample.stereo[2*a]*(1-f)+sample.stereo[2*b]*f)*gain*p.left;
                    r=(sample.stereo[2*a+1]*(1-f)+sample.stereo[2*b+1]*f)*gain*p.right;
                    v.position+=v.increment;
                }
            }
            if(v.tailLeft) {const float gain=static_cast<float>(--v.tailLeft)/v.tailLength*tailGate(v.tailMask);l+=v.tailL*gain;r+=v.tailR*gain;}
            v.lastL=l;v.lastR=r;left+=l;right+=r;
        }
        output.left[frame]=left;output.right[frame]=right;
    }
    uint32_t active=0;for(const auto& v:voices_)active+=v.active || v.tailLeft;active_.store(active,std::memory_order_relaxed);
    blockEvents_={};
}
}

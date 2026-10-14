#include "af_kernel.h"
#include <inttypes.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>

#define CHECK(x) do { if (!(x)) { fprintf(stderr,"FAIL %s:%d: %s\n",__FILE__,__LINE__,#x); exit(1); } } while (0)

static af_config configuration(void) {
    return (af_config){.position_min=0,.position_max=100,.coarse_step=25,.fine_step=5,
        .min_peak_score=100,.min_contrast_span=100,.confirmation_drop=10,.settle_ms=0,.max_age_ms=10,
        .max_moves=128,.max_measurements=128,.max_steps=256,.max_duration_ms=10000,
        .require_measured_position=true,.require_completion=true,.exposure_epoch=0};
}
static af_observation fresh(const af_state *s, uint32_t sequence, uint64_t now, uint32_t score) {
    return (af_observation){.sequence_id=sequence,.command_id_at_acquisition=s->command_id,
        .commanded_position_at_acquisition=s->pending_position,.acquired_at=now,.delivered_at=now,
        .position_known=true,.reported_position=s->pending_position,.completion_known=true,.reported_completion=true,
        .valid=true,.score_known=true,.sharpness=score,.exposure_epoch=s->config.exposure_epoch,.supported_mode=true};
}
static af_action search(af_config c, int32_t peak, bool flat, bool bad_final) {
    af_state s; af_action a;
    CHECK(af_init(&s,&c,0,&a));
    CHECK(a.kind==AF_MOVE_TO && !a.trigger_known && a.target_position==c.position_min);
    for (uint32_t i=1; i<300; i++) {
        int64_t distance=(int64_t)s.pending_position-peak;
        uint32_t score=flat ? 1000 : (uint32_t)(10000-distance*distance);
        if (bad_final && s.phase==2) score=0;
        af_observation o=fresh(&s,i,i,score);
        af_step(&s,&o,&a);
        if (a.kind==AF_MOVE_TO) CHECK(a.target_position>=c.position_min && a.target_position<=c.position_max);
        if (a.terminal) return a;
    }
    CHECK(false); return a;
}
int main(void) {
    af_config c=configuration();
    af_action a=search(c,60,false,false);
    CHECK(a.kind==AF_FINISH && a.target_position==60 && a.reason==AF_CONFIRMED);
    CHECK(a.measurements<=c.max_measurements && a.moves<=c.max_moves && a.steps<=c.max_steps);
    a=search(c,60,true,false); CHECK(a.kind==AF_FAIL && a.reason==AF_LOW_CONFIDENCE);
    a=search(c,60,false,true); CHECK(a.kind==AF_FAIL && a.reason==AF_CONFIRM_FAILED);
    af_state s;
    c.position_min=INT32_MIN; c.position_max=INT32_MAX;
    CHECK(!af_init(&s,&c,0,&a));
    c=configuration(); c.coarse_step=UINT32_MAX; CHECK(!af_init(&s,&c,0,&a));
    c=configuration(); c.fine_step=0; CHECK(!af_init(&s,&c,0,&a));
    c=configuration(); c.position_min=INT32_MAX-100; c.position_max=INT32_MAX;
    a=search(c,INT32_MAX-40,false,false); CHECK(a.kind==AF_FINISH && a.target_position==INT32_MAX-40);
    c=configuration(); c.position_min=INT32_MIN; c.position_max=INT32_MIN+100;
    a=search(c,INT32_MIN+60,false,false); CHECK(a.kind==AF_FINISH && a.target_position==INT32_MIN+60);
    c=configuration(); CHECK(af_init(&s,&c,0,&a));
    af_observation o=fresh(&s,1,1,1000); o.command_id_at_acquisition=0;
    af_step(&s,&o,&a); CHECK(a.reason==AF_COMMAND_MISMATCH && s.measurements==0);
    o=fresh(&s,2,100,1000); o.acquired_at=1;
    af_step(&s,&o,&a); CHECK(a.reason==AF_STALE && s.measurements==0);
    o=fresh(&s,3,101,1000); o.reported_completion=false;
    af_step(&s,&o,&a); CHECK(a.reason==AF_WAIT_COMPLETION && s.measurements==0);
    o=fresh(&s,4,102,1000); o.position_known=false;
    af_step(&s,&o,&a); CHECK(a.reason==AF_WAIT_POSITION && s.measurements==0);
    o=fresh(&s,5,103,1000); o.reported_position=1;
    af_step(&s,&o,&a); CHECK(a.reason==AF_WAIT_POSITION && s.measurements==0);
    o=fresh(&s,6,104,1000); o.valid=false; o.score_known=false;
    af_step(&s,&o,&a); CHECK(a.reason==AF_INVALID && s.measurements==0);
    c=configuration(); c.require_measured_position=false; c.require_completion=false; CHECK(af_init(&s,&c,0,&a));
    o=fresh(&s,1,1,1000); o.position_known=false; o.completion_known=false;
    af_step(&s,&o,&a); CHECK(s.measurements==1 && a.kind==AF_MOVE_TO);
    af_step(&s,&o,&a); CHECK(a.reason==AF_DUPLICATE && s.measurements==1);
    o=fresh(&s,2,0,1000); af_step(&s,&o,&a); CHECK(a.kind==AF_FAIL && a.reason==AF_CLOCK);
    CHECK(af_init(&s,&c,UINT64_MAX-5,&a));
    o=fresh(&s,1,UINT64_MAX-4,1000); af_step(&s,&o,&a); CHECK(a.kind==AF_MOVE_TO);
    o=fresh(&s,2,0,1000); af_step(&s,&o,&a); CHECK(a.reason==AF_CLOCK);
    c=configuration(); CHECK(af_init(&s,&c,0,&a)); s.command_id=UINT32_MAX;
    o=fresh(&s,1,1,1000); af_step(&s,&o,&a); CHECK(a.reason==AF_BOUNDS && s.command_id==UINT32_MAX);
    c.max_moves=1; CHECK(af_init(&s,&c,0,&a));
    o=fresh(&s,1,1,1000); af_step(&s,&o,&a); CHECK(a.reason==AF_MOVE_LIMIT && s.moves==1);
    c=configuration(); c.max_measurements=1; CHECK(af_init(&s,&c,0,&a));
    o=fresh(&s,1,1,1000); af_step(&s,&o,&a);
    o=fresh(&s,2,2,1000); af_step(&s,&o,&a); CHECK(a.reason==AF_MEASUREMENT_LIMIT && s.measurements==1);
    c=configuration(); c.max_steps=1; CHECK(af_init(&s,&c,0,&a));
    o=fresh(&s,1,1,1000); o.valid=false; af_step(&s,&o,&a);
    o=fresh(&s,2,2,1000); af_step(&s,&o,&a); CHECK(a.reason==AF_STEP_LIMIT && s.steps==1);
    c=configuration(); c.max_duration_ms=2; CHECK(af_init(&s,&c,0,&a));
    o=fresh(&s,1,2,1000); af_step(&s,&o,&a); CHECK(a.reason==AF_TIME_LIMIT);
    c=configuration(); CHECK(af_init(&s,&c,0,&a));
    o=fresh(&s,1,1,1000); o.cancelled=true; af_step(&s,&o,&a); CHECK(a.kind==AF_HAND_BACK && a.reason==AF_CANCELLED);
    uint32_t moves=s.moves; af_step(&s,&o,&a); CHECK(a.reason==AF_TERMINAL && s.moves==moves);
    CHECK(af_init(&s,&c,0,&a)); o=fresh(&s,1,1,1000); o.supported_mode=false;
    af_step(&s,&o,&a); CHECK(a.kind==AF_HAND_BACK && a.reason==AF_MODE);
    CHECK(af_init(&s,&c,0,&a)); o=fresh(&s,1,1,1000); o.exposure_epoch=1;
    af_step(&s,&o,&a); CHECK(a.kind==AF_HAND_BACK && a.reason==AF_EXPOSURE);
    CHECK(af_init(&s,&c,0,&a)); o=fresh(&s,1,1,1000); o.acquired_at=2;
    af_step(&s,&o,&a); CHECK(a.reason==AF_CLOCK);
    printf("PASS: bounded synthetic C decisions; host sizeof(state)=%zu, sizeof(action)=%zu\n",sizeof(s),sizeof(a));
    return 0;
}

#define _POSIX_C_SOURCE 200809L
/* Metadata-only H0 framing probe. No decoder, camera, network or upstream code. */
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

#define INPUT_SIZE 38405148u
#define HEADER_SIZE 532u
static const char expected_digest[] = "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490";
static volatile sig_atomic_t digest_child;

static void deadline(int signal_number)
{
    (void)signal_number;
    if(digest_child>0) (void)kill((pid_t)digest_child,SIGKILL);
    _exit(124);
}

static void reject(const char *message)
{
    if(digest_child>0) (void)kill((pid_t)digest_child,SIGKILL);
    fprintf(stderr,"framing-probe: %s\n",message); exit(1);
}
static void write_all(int fd,const unsigned char *bytes,size_t length)
{
    while(length) {
        ssize_t count=write(fd,bytes,length);
        if(count<0 && errno==EINTR) continue;
        if(count<=0) reject("bounded digest pipe write failed");
        bytes+=(size_t)count; length-=(size_t)count;
    }
}
/* Hash the already observed buffer through the pinned host coreutils helper,
 * never by reopening the source filename. Child pipes/exit are checked. */
static void hash_bytes(const unsigned char *bytes,size_t length,char digest[65])
{
    int input[2],output[2];
    if(pipe(input)||pipe(output)) reject("digest pipe creation failed");
    pid_t child=fork(); if(child<0) reject("digest fork failed");
    if(child==0) {
        if(dup2(input[0],STDIN_FILENO)<0 || dup2(output[1],STDOUT_FILENO)<0) _exit(125);
        close(input[0]); close(input[1]); close(output[0]); close(output[1]);
        execlp("sha256sum","sha256sum",(char *)NULL); _exit(126);
    }
    digest_child=(sig_atomic_t)child;
    close(input[0]); close(output[1]); write_all(input[1],bytes,length);
    if(close(input[1])) reject("digest input close failed");
    char result[128]; size_t used=0;
    for(;;) {
        if(used==sizeof result) reject("digest output cap exceeded");
        ssize_t got=read(output[0],result+used,sizeof result-used);
        if(got<0 && errno==EINTR) continue;
        if(got<0) reject("digest output read failed");
        if(!got) break;
        used+=(size_t)got;
    }
    if(close(output[0])) reject("digest output close failed");
    int status=0; pid_t waited;
    do { waited=waitpid(child,&status,0); } while(waited<0 && errno==EINTR);
    digest_child=0;
    if(waited!=child || !WIFEXITED(status) || WEXITSTATUS(status)!=0 || used!=68 || memcmp(result+64,"  -\n",4)) reject("digest helper failed");
    for(size_t index=0;index<64;++index)
        if(!((result[index]>='0'&&result[index]<='9')||(result[index]>='a'&&result[index]<='f'))) reject("invalid digest helper output");
    memcpy(digest,result,64); digest[64]='\0';
}
static uint32_t sum_bytes(const unsigned char *bytes,size_t length)
{ uint32_t sum=0; for(size_t index=0;index<length;++index) sum+=bytes[index]; return sum; }
typedef struct { const unsigned char *bytes; size_t length; } Marker;
static const Marker markers[] = {
    {(const unsigned char *)"RIHR",4}, {(const unsigned char *)"LENGTH=",7},
    {(const unsigned char *)"ND1",3}, {(const unsigned char *)"IPL",3},
    {(const unsigned char *)"PTBL",4}, {(const unsigned char *)"A9262",5},
    {(const unsigned char *)"\x1f\x8b\x08",3}, {(const unsigned char *)"\xfd" "7zXZ\x00",6},
};
static void emit_buffer(const unsigned char *bytes,size_t length,const char *digest)
{
    printf("{\"bytes\":%zu,\"sha256\":\"%s\",\"byte_sum_mod_2_32\":\"0x%08" PRIx32 "\",\"markers\":{",length,digest,sum_bytes(bytes,length));
    for(size_t index=0;index<sizeof markers/sizeof markers[0];++index) {
        Marker marker=markers[index]; size_t cursor=0,count=0,offsets[32];
        while(cursor+marker.length<=length) {
            const unsigned char *found=memchr(bytes+cursor,marker.bytes[0],length-cursor-marker.length+1);
            if(!found) break;
            size_t position=(size_t)(found-bytes);
            if(!memcmp(found,marker.bytes,marker.length)) {
                if(count<32) offsets[count]=position;
                ++count; cursor=position+marker.length;
            } else cursor=position+1;
        }
        printf("%s\"",index?",":"");
        for(size_t part=0;part<marker.length;++part) printf("%02x",marker.bytes[part]);
        printf("\":{\"count\":%zu,\"first_offsets\":[",count);
        for(size_t part=0;part<count&&part<32;++part) printf("%s%zu",part?",":"",offsets[part]);
        printf("]}");
    }
    printf("}}");
}
int main(int argc,char **argv)
{
    struct rlimit memory={256u*1024u*1024u,256u*1024u*1024u},cpu={10,10};
    if(setrlimit(RLIMIT_AS,&memory)||setrlimit(RLIMIT_CPU,&cpu)) reject("resource limit setup failed");
    struct sigaction action;
    memset(&action,0,sizeof action); action.sa_handler=deadline;
    if(sigemptyset(&action.sa_mask)||sigaction(SIGALRM,&action,NULL)) reject("deadline handler setup failed");
    alarm(15);
    if(argc==2 && !strcmp(argv[1],"--self-test")) {
        char empty[65],abc[65],repeated[65]; unsigned char synthetic[160];
        hash_bytes((const unsigned char *)"",0,empty); hash_bytes((const unsigned char *)"abc",3,abc);
        if(strcmp(empty,"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855") ||
           strcmp(abc,"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")) reject("independent SHA256 vectors failed");
        for(size_t index=0;index<40;++index) memcpy(synthetic+index*4,"RIHR",4);
        hash_bytes(synthetic,sizeof synthetic,repeated);
        printf("{\"sha256_vectors\":\"passed\",\"synthetic\":"); emit_buffer(synthetic,sizeof synthetic,repeated); printf("}\n");
        return 0;
    }
    if(argc!=2) reject("usage: framing-probe EXACT_PRIVATE_DAT");
    struct stat before,after;
    if(lstat(argv[1],&before)||!S_ISREG(before.st_mode)||before.st_size!=INPUT_SIZE) reject("expected exact regular DAT; symlinks/special files rejected");
    int fd=open(argv[1],O_RDONLY|O_NOFOLLOW|O_NONBLOCK|O_CLOEXEC);
    if(fd<0) reject("nofollow nonblocking input open failed");
    if(fstat(fd,&after)||!S_ISREG(after.st_mode)||after.st_size!=INPUT_SIZE) reject("input changed type/size");
    unsigned char *raw=malloc(INPUT_SIZE+1u),*payload=malloc(INPUT_SIZE-HEADER_SIZE);
    if(!raw||!payload) reject("bounded input allocation failed");
    size_t used=0;
    while(used<INPUT_SIZE+1u) {
        ssize_t got=read(fd,raw+used,INPUT_SIZE+1u-used);
        if(got<0 && errno==EINTR) continue;
        if(got<0) reject("bounded input read failed");
        if(!got) break;
        used+=(size_t)got;
    }
    if(close(fd)) reject("input close failed");
    if(used!=INPUT_SIZE) reject("input size mismatch");
    char raw_digest[65],payload_digest[65]; hash_bytes(raw,used,raw_digest);
    if(strcmp(raw_digest,expected_digest)) reject("input size/hash mismatch");
    size_t payload_length=INPUT_SIZE-HEADER_SIZE;
    for(size_t index=0;index<payload_length;++index) payload[index]=(unsigned char)(raw[index+HEADER_SIZE]^255u);
    hash_bytes(payload,payload_length,payload_digest);
    uint32_t checksum=(uint32_t)raw[524]|(uint32_t)raw[525]<<8|(uint32_t)raw[526]<<16|(uint32_t)raw[527]<<24;
    printf("{\"raw_container\":"); emit_buffer(raw,used,raw_digest);
    printf(",\"complemented_payload\":"); emit_buffer(payload,payload_length,payload_digest);
    printf(",\"raw_payload_byte_sum_mod_2_32\":\"0x%08" PRIx32 "\",\"header_checksum_word_unvalidated\":\"0x%08" PRIx32 "\"}\n",sum_bytes(raw+HEADER_SIZE,payload_length),checksum);
    free(payload); free(raw); return 0;
}

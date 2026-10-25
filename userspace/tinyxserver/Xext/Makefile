include	../standard_definitions.mk

#-DMITMISC -DXTRAP -DXCMISC -DFONTCACHE
DEFS= -DBIGREQS 

CFLAGS= -I../include -I../mi -I../fb -I../hw/kdrive -I../render $(COMMONDEFS) $(DEFS)

#OBJS=$(wildcard *.c)
ifeq ($(PANORAMIX),-DPANORAMIX)
OBJS1=panoramiX.c panoramiXSwap.c panoramiXprocs.c        
endif

ifeq ($(SHAPE),-DSHAPE)
OBJS2=shape.c
endif

ifeq ($(DPMS),-DDPMSExtension)
OBJS3=dpms.c
endif

OBJS=$(OBJS1) $(OBJS2) $(OBJS3) bigreq.c saver.c shm.c sleepuntil.c sync.c xcmisc.c


libICE.a:	$(OBJS:.c=.o) 
	ar cr libext.a $(OBJS:.c=.o) 
clean:
	rm -f *.o *.a
install:	libext.a
	cp -f libext.a $(LIBDIR)
depend:
	gccmakedep -- $(CFLAGS) -- *.c